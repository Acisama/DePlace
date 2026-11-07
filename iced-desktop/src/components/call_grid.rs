//! `CallGrid`: Discord-style grid for call tiles.
//!
//! Every tile keeps a fixed 16:9 aspect ratio. The column count is chosen
//! by trying every possible count from 1 to the number of items and keeping
//! whichever produces the largest tile that still fits the available space.
//! A trailing row with fewer items than the column count is centered
//! horizontally.
//!
//! Tiles ease into their new slot whenever the *set* of items changes (a
//! member joins or leaves) — never on a plain resize, which just reflows
//! instantly. A newly joined member grows in from nothing. When a member
//! leaves, a clone of it is kept around and re-rendered, shrinking away in
//! place for the duration of the transition, so its tile fades out of
//! existence rather than popping out.
//!
//! The interpolation itself happens in `layout`, same as this fork's own
//! `widget::transition::Transition` does for per-frame size changes. Since
//! a plain `shell.request_redraw` doesn't cause `layout` to run again on
//! its own, `update` additionally calls `shell.invalidate_layout` every
//! frame a transition or a leaving ghost is still in flight, which is the
//! supported way to get `layout` re-invoked without rebuilding the view
//! (and therefore without asking the rest of the app to redraw).
//! `shell.invalidate_layout` triggers a full top-down relayout rather than
//! a targeted one, but that's still far cheaper than a `view` rebuild, and
//! it only runs for the handful of frames an animation is in flight.

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{Meta, Operation, Tree, tree};
use iced::advanced::{Shell, Widget};
use iced::{
    Element, Event, Length, Padding, Pixels, Rectangle, Renderer, Size, Vector, Widget as _, mouse,
    window,
};

use crate::common::DePlaceWidget;

pub struct CallGrid<T, Key, Message, Content> {
    items: Vec<T>,
    key: Box<dyn Fn(&T) -> Key>,
    render: Box<dyn Fn(T) -> Content>,
    spacing: f32,
    padding: Padding,
    width: Length,
    height: Length,
    aspect_ratio: f32,
    transition_duration: Duration,

    // Resolved by `diff` and consumed by `layout` for this frame.
    children: Vec<Content>,
    // `diff` drains `items`, so it must run at most once per instance: an
    // ancestor `lazy` widget re-diffs its *cached, unchanged* element on
    // every unrelated rebuild, not just when its dependency hash actually
    // changes, which would otherwise make every such call see an empty
    // `items` and wrongly conclude the whole roster just left.
    diffed: bool,

    _marker: std::marker::PhantomData<Message>,
}

/// Builds a [`CallGrid`] from `items`, deriving a continuity key and a
/// rendered element for each one.
pub fn call_grid<T, Key, Message, Content>(
    items: impl IntoIterator<Item = T>,
    key: impl Fn(&T) -> Key + 'static,
    render: impl Fn(T) -> Content + 'static,
) -> CallGrid<T, Key, Message, Content>
where
    T: Clone + 'static,
    Key: Clone + PartialEq + 'static,
    Content: DePlaceWidget<Message>,
{
    CallGrid {
        items: items.into_iter().collect(),
        key: Box::new(key),
        render: Box::new(render),
        spacing: 0.0,
        padding: Padding::ZERO,
        width: Length::Fill,
        height: Length::Fill,
        aspect_ratio: 16.0 / 9.0,
        transition_duration: Duration::from_millis(250),
        children: Vec::new(),
        diffed: false,
        _marker: std::marker::PhantomData,
    }
}

impl<T, Key, Message, Content> CallGrid<T, Key, Message, Content> {
    pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
        self.spacing = amount.into().0;
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn aspect_ratio(mut self, ratio: f32) -> Self {
        self.aspect_ratio = ratio;
        self
    }

    /// How long a reflow (triggered by a join or a leave) takes to settle.
    pub fn transition_duration(mut self, duration: Duration) -> Self {
        self.transition_duration = duration;
        self
    }
}

/// A member that disappeared from `items`; kept around, frozen, so its tile
/// can keep being rendered for the remainder of the transition.
struct Leaving<Key, T> {
    key: Key,
    item: T,
    position: Vector,
    size: Size,
    started: Instant,
}

struct State<Key, T> {
    // The members (and their last known data) as of the most recent `diff`.
    // Lives here rather than on the widget itself because a fresh
    // `CallGrid` is built on every real rebuild, so `self` never carries
    // anything over between one `diff` and the next - only `tree.state`
    // does.
    items: Vec<(Key, T)>,
    // Positions produced by the most recent `layout` call, used both as a
    // "last known good" baseline for the next transition and to find where
    // a departing member was sitting.
    last_positions: Vec<(Key, Vector)>,
    last_tile_size: Size,
    // Position and tile size frozen at the start of the in-flight
    // transition, lerped from.
    from: Vec<(Key, Vector)>,
    size_from: Size,
    transition: Option<Instant>,
    leaving: Vec<Leaving<Key, T>>,
}

impl<Key, T> Default for State<Key, T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            last_positions: Vec::new(),
            last_tile_size: Size::ZERO,
            from: Vec::new(),
            size_from: Size::ZERO,
            transition: None,
            leaving: Vec::new(),
        }
    }
}

impl<Key, T> State<Key, T> {
    fn is_animating(&self, transition_duration: Duration) -> bool {
        self.transition
            .is_some_and(|started| started.elapsed() < transition_duration)
            || self
                .leaving
                .iter()
                .any(|l| l.started.elapsed() < transition_duration)
    }
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn progress(started: Instant, duration: Duration) -> f32 {
    smoothstep((started.elapsed().as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0))
}

fn lerp_vector(from: Vector, to: Vector, t: f32) -> Vector {
    Vector::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
}

fn lerp_size(from: Size, to: Size, t: f32) -> Size {
    Size::new(
        from.width + (to.width - from.width) * t,
        from.height + (to.height - from.height) * t,
    )
}

fn center_of(position: Vector, size: Size) -> Vector {
    Vector::new(position.x + size.width / 2.0, position.y + size.height / 2.0)
}

fn top_left(center: Vector, size: Size) -> Vector {
    Vector::new(center.x - size.width / 2.0, center.y - size.height / 2.0)
}

/// Picks the column count that maximizes tile size, and the resulting
/// tile `Size`, for `n` items laid out in `available` space.
fn best_grid(n: usize, available: Size, spacing: f32, aspect_ratio: f32) -> (usize, Size) {
    let mut best_columns = 1;
    let mut best_tile = Size::ZERO;

    for columns in 1..=n {
        let rows = n.div_ceil(columns);

        let cell_width = (available.width - spacing * (columns as f32 - 1.0)) / columns as f32;
        let cell_height = (available.height - spacing * (rows as f32 - 1.0)) / rows as f32;

        if cell_width <= 0.0 || cell_height <= 0.0 {
            continue;
        }

        let width = cell_width.min(cell_height * aspect_ratio);

        if width > best_tile.width {
            best_columns = columns;
            best_tile = Size::new(width, width / aspect_ratio);
        }
    }

    (best_columns, best_tile)
}

/// The tile size and the (centered) target position of each of `n` tiles
/// within `available` space. Degenerates gracefully to an empty `Vec` when
/// `n == 0`.
fn grid_positions(
    n: usize,
    available: Size,
    spacing: f32,
    aspect_ratio: f32,
) -> (Size, Vec<Vector>) {
    let (columns, tile) = best_grid(n, available, spacing, aspect_ratio);
    let rows = n.div_ceil(columns);
    let mut positions = Vec::with_capacity(n);

    for row in 0..rows {
        let start = row * columns;
        let end = (start + columns).min(n);
        let count = end - start;

        let row_width = count as f32 * tile.width + (count as f32 - 1.0).max(0.0) * spacing;
        let x0 = (available.width - row_width) / 2.0;
        let y = row as f32 * (tile.height + spacing);

        for offset in 0..count {
            positions.push(Vector::new(x0 + offset as f32 * (tile.width + spacing), y));
        }
    }

    (tile, positions)
}

impl<T, Key, Message, Content> Meta for CallGrid<T, Key, Message, Content> {}

impl<T, Key, Message, Content> Widget<Message, iced::Theme, Renderer>
    for CallGrid<T, Key, Message, Content>
where
    T: Clone + 'static,
    Key: Clone + PartialEq + 'static,
    Content: DePlaceWidget<Message>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Key, T>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Key, T>::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        if std::mem::replace(&mut self.diffed, true) {
            return;
        }

        let member_pairs: Vec<(Key, T)> = std::mem::take(&mut self.items)
            .into_iter()
            .map(|item| ((self.key)(&item), item))
            .collect();

        let new_keys: Vec<Key> = member_pairs.iter().map(|(key, _)| key.clone()).collect();

        let state = tree.state.downcast_mut::<State<Key, T>>();

        let left: Vec<(Key, T)> = state
            .items
            .iter()
            .filter(|(key, _)| !new_keys.contains(key))
            .cloned()
            .collect();
        let joined = new_keys
            .iter()
            .any(|key| !state.items.iter().any(|(k, _)| k == key));

        if joined || !left.is_empty() {
            for (key, item) in &left {
                if let Some(position) = state
                    .last_positions
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, p)| *p)
                {
                    state.leaving.push(Leaving {
                        key: key.clone(),
                        item: item.clone(),
                        position,
                        size: state.last_tile_size,
                        started: Instant::now(),
                    });
                }
            }

            state.from = state.last_positions.clone();
            state.size_from = state.last_tile_size;
            state.transition = Some(Instant::now());
        }

        state.leaving.retain(|l| {
            l.started.elapsed() < self.transition_duration && !new_keys.contains(&l.key)
        });

        let mut children: Vec<Content> =
            Vec::with_capacity(member_pairs.len() + state.leaving.len());

        for (_, item) in &member_pairs {
            children.push((self.render)(item.clone()));
        }

        for leaving in &state.leaving {
            children.push((self.render)(leaving.item.clone()));
        }

        tree::diff_children_custom(&mut tree.children, &mut children, Tree::diff, |content| {
            Tree::new(content)
        });

        state.items = member_pairs;
        self.children = children;
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let limits = limits.shrink(self.padding);
        let available = limits.resolve(self.width, self.height, Size::ZERO);

        // `targets`/`leaving.position` are relative to the padding-shrunk
        // content area, but `translation` is read relative to this widget's
        // own (unshrunk) origin, so it needs the padding added back in.
        let padding_offset = Vector::new(self.padding.left, self.padding.top);

        let state = tree.state.downcast_mut::<State<Key, T>>();

        let n_real = state.items.len();
        let (tile, targets) = grid_positions(n_real, available, self.spacing, self.aspect_ratio);

        let t = state
            .transition
            .map_or(1.0, |started| progress(started, self.transition_duration));

        for (i, ((key, _), target)) in state.items.iter().zip(&targets).enumerate() {
            let from = state.from.iter().find(|(k, _)| k == key).map(|(_, p)| *p);

            // A member with no `from` entry wasn't around when this
            // transition started, i.e. it just joined: grow it in from
            // nothing instead of popping in at full size. A continuing
            // member eases both its position and its tile size (which can
            // change as the grid reflows) towards the target. Either way,
            // interpolate the box's *center* rather than its top-left
            // corner, and derive the corner from that afterwards, so a
            // tile grows/shrinks around its own center instead of its
            // corner - and, since `size == size_from` and `center ==
            // center_of(from, size_from)` at `t == 0`, the derived corner
            // is exactly `from` with no jump at the start of the ease.
            let target_center = center_of(*target, tile);

            let (center, size) = match from {
                Some(from) if t < 1.0 => {
                    let size = lerp_size(state.size_from, tile, t);
                    let center =
                        lerp_vector(center_of(from, state.size_from), target_center, t);
                    (center, size)
                }
                None if t < 1.0 => (target_center, lerp_size(Size::ZERO, tile, t)),
                _ => (target_center, tile),
            };

            let child_limits = layout::Limits::new(size, size);
            self.children[i].layout(&mut tree.children[i], renderer, &child_limits);
            tree.children[i].translation = top_left(center, size) + padding_offset;
        }

        state.last_positions = state
            .items
            .iter()
            .zip(&targets)
            .map(|((key, _), position)| (key.clone(), *position))
            .collect();
        state.last_tile_size = tile;

        for (i, leaving) in state.leaving.iter().enumerate() {
            // Leaving members don't slide anywhere - they just shrink away
            // around their own center.
            let t = progress(leaving.started, self.transition_duration);
            let size = lerp_size(leaving.size, Size::ZERO, t);
            let center = center_of(leaving.position, leaving.size);

            let child_limits = layout::Limits::new(size, size);
            self.children[n_real + i].layout(
                &mut tree.children[n_real + i],
                renderer,
                &child_limits,
            );
            tree.children[n_real + i].translation = top_left(center, size) + padding_offset;
        }

        tree.size = available.expand(self.padding);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for (child, (layout, tree)) in self.children.iter().zip(layout.iter(&tree.children)) {
            child.draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds(), viewport);
        operation.traverse(&mut |operation| {
            for (child, (layout, tree)) in self
                .children
                .iter_mut()
                .zip(layout.iter_mut(&mut tree.children))
            {
                child.operate(tree, layout, viewport, renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for (child, (layout, tree)) in self
            .children
            .iter_mut()
            .zip(layout.iter_mut(&mut tree.children))
        {
            child.update(tree, event, layout, cursor, renderer, shell, viewport);
        }

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let state = tree.state.downcast_ref::<State<Key, T>>();

            if state.is_animating(self.transition_duration) {
                // A plain `request_redraw` only schedules another `draw` -
                // `layout` (where the interpolation above happens) isn't
                // re-run without this, same as `widget::transition::Transition`
                // has to invalidate layout itself for a per-frame size change.
                shell.invalidate_layout();
                shell.request_redraw();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(layout.iter(&tree.children))
            .map(|(child, (layout, tree))| {
                child.mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, T, Key, Message, Content> From<CallGrid<T, Key, Message, Content>>
    for Element<'a, Message, iced::Theme, Renderer>
where
    T: Clone + 'static,
    Key: Clone + PartialEq + 'static,
    Content: DePlaceWidget<Message> + 'a,
    Message: 'a,
{
    fn from(widget: CallGrid<T, Key, Message, Content>) -> Self {
        widget.boxed()
    }
}
