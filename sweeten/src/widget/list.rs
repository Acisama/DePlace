//! A virtualized list that only materializes visible items.
//!
//! Ported from iced's `feat/list-widget-redux` branch. The [`List`]
//! widget takes a [`Content<T>`] (the full data) and a view closure,
//! but only instantiates [`Element`]s for the items currently in the
//! viewport. This makes it suitable for large or unbounded data sets
//! where creating a widget per row would be too expensive.
//!
//! Modified from the `List` widget by Héctor Ramón Jiménez (hecrj),
//! originally on iced's `feat/list-widget-reloaded` branch.
//! Licensed under the MIT license.

use iced_core::widget::Meta;
use iced_core::widget::Operation;
use iced_core::widget::operation::Outcome;
use iced_widget::Id;
use indexmap::IndexMap;

use crate::core::layout;
use crate::core::mouse;
use crate::core::overlay;
use crate::core::renderer;
use crate::core::widget;
use crate::core::widget::tree::{self, Tree};
use crate::core::window;
use crate::core::{
    self, Element, Event, Layout, Length, Pixels, Point, Rectangle, Shell,
    Size, Vector, Widget,
};

use std::borrow::Borrow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::rc::Rc;

/// A virtualized list widget.
///
/// Only the items visible in the current viewport are materialized
/// into [`Element`]s, so scrolling through thousands of rows stays
/// responsive. The backing data lives in a [`Content<T>`] that
/// tracks insertions, removals, and mutations so the widget tree
/// stays in sync without a full rebuild each frame.
///
/// [`List`] owns its [`Content<T>`] (cloning one is cheap — see
/// [`Content<T>`]'s docs), which lets the resulting [`Element`] be
/// `'static` and used with [`iced::widget::lazy`].
#[allow(missing_debug_implementations)]
pub struct List<'a, S: Eq + Hash + Clone, T, Message, Theme, Renderer> {
    id: Option<Id>,
    content: Content<S, T>,
    spacing: f32,
    view_item: Box<
        dyn Fn(usize, S, Rc<T>) -> Element<'a, Message, Theme, Renderer> + 'a,
    >,
    visible_elements: Vec<Element<'a, Message, Theme, Renderer>>,
}

impl<'a, S: Eq + Hash + Clone, T, Message, Theme, Renderer>
    List<'a, S, T, Message, Theme, Renderer>
{
    /// Creates a new [`List`] backed by `content`, using `view_item`
    /// to materialize each visible row.
    pub fn new(
        content: Content<S, T>,
        view_item: impl Fn(usize, S, Rc<T>) -> Element<'a, Message, Theme, Renderer>
        + 'a,
    ) -> Self {
        Self {
            id: None,
            content,
            spacing: 0.0,
            view_item: Box::new(view_item),
            visible_elements: Vec::new(),
        }
    }

    /// Sets the [`Id`] of the [`List`].
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the vertical spacing _between_ elements.
    ///
    /// Custom margins per element do not exist in iced. You should use this
    /// method instead! While less flexible, it helps you keep spacing between
    /// elements consistent.
    pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
        self.spacing = amount.into().0;
        self
    }
}

struct State {
    last_limits: layout::Limits,
    // Identity of the `Content` stream last laid out (the address of its
    // shared `is_new` cell, which -- unlike `items` -- is never replaced by
    // COW and is never left the *same* for two different `Content::new`
    // instances). `List::State` is keyed by widget-tree *position*, so
    // swapping in a different logical `Content` at the same position (e.g.
    // switching rooms) must still force a full reset even if that specific
    // `Content`'s own `is_new` already happens to be `false` from an
    // earlier viewing -- otherwise stale `visible_layouts` indices from the
    // previous content's (possibly longer) item list get used against the
    // new, shorter one and panic.
    last_identity: usize,
    visible_layouts: Vec<(usize, Layout, Tree)>,
    size: Size,
    offsets: Vec<f32>,
    spacing: f32,
    widths: Vec<f32>,
    task: Task,
    visible_outdated: bool,
}

enum Task {
    Idle,
    Computing {
        current: usize,
        offsets: Vec<f32>,
        widths: Vec<f32>,
        size: Size,
    },
}

impl State {
    fn recompute(&mut self, size: usize) {
        let mut offsets = Vec::with_capacity(size + 1);
        offsets.push(0.0);

        self.task = Task::Computing {
            current: 0,
            offsets,
            widths: Vec::with_capacity(size),
            size: Size::ZERO,
        };
        self.visible_layouts.clear();
    }
}

impl<S: Eq + Hash + Clone + Debug, T, Message, Theme, Renderer> Meta
    for List<'_, S, T, Message, Theme, Renderer>
where
    Renderer: core::Renderer,
{
}

impl<'a, S: Eq + Hash + Clone + Debug, T, Message, Theme, Renderer>
    Widget<Message, Theme, Renderer>
    for List<'a, S, T, Message, Theme, Renderer>
where
    Renderer: core::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            last_limits: layout::Limits::new(Size::ZERO, Size::ZERO),
            last_identity: 0,
            visible_layouts: Vec::new(),
            size: Size::ZERO,
            offsets: vec![0.0],
            widths: Vec::new(),
            task: Task::Idle,
            visible_outdated: false,
            spacing: 0.0,
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let loose_limits = limits.loose();

        // On an axis with no upper bound the maximum boundary is only a
        // hint: no item's measurement can depend on it. A vertical
        // `scrollable` makes the height axis unbounded, so when only the
        // available height changes the list must NOT re-measure everything.
        // The timeline input next to a chat grows a line every time the
        // user wraps text or presses shift+enter, shrinking the
        // scrollable's viewport height; re-measuring every timeline item
        // on each of those (a whole-application relayout per 50-item
        // batch, requested every frame) made typing lag for seconds and
        // flood the runtime with "3 consecutive RedrawRequested events
        // produced invalidation" warnings.
        //
        // When the height axis is bounded on either side (old or new),
        // fall back to the full comparison: a height change can then
        // affect how the list's own height resolves.
        let height_unbounded =
            state.last_limits.infinite.height && loose_limits.infinite.height;

        let limits_changed = if height_unbounded {
            state.last_limits.max.width != loose_limits.max.width
                || state.last_limits.compression.width
                    != loose_limits.compression.width
                || state.last_limits.infinite.width
                    != loose_limits.infinite.width
                || state.last_limits.compression.height
                    != loose_limits.compression.height
        } else {
            state.last_limits != loose_limits
        };

        if limits_changed {
            state.last_limits = loose_limits;
            state.recompute(self.content.len());
            self.content.changes.borrow_mut().clear();
        }

        let identity = Rc::as_ptr(&self.content.is_new) as *const () as usize;
        if state.last_identity != identity {
            state.last_identity = identity;
            state.recompute(self.content.len());
            self.content.changes.borrow_mut().clear();
        }

        let mut changes = self.content.changes.borrow_mut();

        // Set when queued content changes invalidated an in-flight
        // measurement above; see the `Task::Computing` arm.
        let mut finish_restarted_measurement = false;

        match state.task {
            Task::Idle => {
                while let Some(change) = changes.pop_front() {
                    // A queued `Change` records two sets of coordinates:
                    // `current` indexes the already-mutated `items`, and
                    // `original` indexes this widget's (stale) offsets and
                    // widths. If the queue describes a state this widget no
                    // longer matches -- e.g. a mutation happened while
                    // another clone of the `Content` kept this element's
                    // `items` snapshot behind `Rc::make_mut` -- replaying
                    // blindly would index out of bounds. Rebuild from
                    // scratch instead.
                    let consistent = match change {
                        Change::Updated { original, current } => {
                            current < self.content.len()
                                && original < state.widths.len()
                                && original < state.offsets.len() - 1
                        }
                        Change::Removed { original, .. } => {
                            original < state.widths.len()
                        }
                        Change::Inserted { original, current } => {
                            current < self.content.len()
                                && original <= state.widths.len()
                        }
                    };

                    if !consistent {
                        changes.clear();
                        state.recompute(self.content.len());
                        break;
                    }

                    match change {
                        Change::Updated { original, current } => {
                            let (key, value) = self
                                .content
                                .items
                                .get_index(current)
                                .expect("Indexing should work");
                            let mut new_element = (self.view_item)(
                                current,
                                key.clone(),
                                value.clone(),
                            );

                            let visible_index = state
                                .visible_layouts
                                .iter_mut()
                                .position(|(i, _, _)| *i == original);

                            let mut new_tree;

                            // Update if visible
                            let tree =
                                if let Some(visible_index) = visible_index {
                                    let (_i, _layout, tree) = &mut state
                                        .visible_layouts[visible_index];

                                    tree.diff(&mut new_element);
                                    state.visible_outdated = true;

                                    tree
                                } else {
                                    new_tree = Tree::new(&new_element);
                                    new_element.diff(&mut new_tree);

                                    &mut new_tree
                                };

                            new_element.layout(
                                tree,
                                renderer,
                                &state.last_limits,
                            );

                            let new_layout = Layout::new(tree.size);
                            let new_size = new_layout.size();

                            let mut height_difference = 0.0;
                            // One offset per item plus the final total,
                            // so the last item's delta still folds in.
                            if original + 1 < state.offsets.len() {
                                height_difference = new_size.height
                                    - (state.offsets[original + 1]
                                        - state.offsets[original]);

                                for offset in &mut state.offsets[original + 1..]
                                {
                                    *offset += height_difference;
                                }
                            }

                            let original_width = state.widths[original];
                            state.widths[original] = new_size.width;

                            if let Some(visible_index) = visible_index {
                                state.visible_layouts[visible_index].1 =
                                    new_layout;

                                for (i, layout, _) in
                                    &mut state.visible_layouts[visible_index..]
                                {
                                    *layout = layout
                                        .move_to((0.0, state.offsets[*i]));
                                }
                            } else if let Some(first_visible) =
                                state.visible_layouts.first()
                            {
                                let first_visible_index = first_visible.0;
                                if original < first_visible_index {
                                    for (i, layout, _) in
                                        &mut state.visible_layouts[..]
                                    {
                                        *layout = layout
                                            .move_to((0.0, state.offsets[*i]));
                                    }
                                }
                            }

                            state.size.height += height_difference;

                            if original_width == state.size.width {
                                state.size.width = state.widths.iter().fold(
                                    0.0,
                                    |current, candidate| {
                                        current.max(*candidate)
                                    },
                                );
                            }
                        }
                        Change::Removed { original, .. } => {
                            let height = state.offsets
                                [(original + 1).min(state.offsets.len() - 1)]
                                - state.offsets[original];

                            let original_width = state.widths.remove(original);
                            let _ = state.offsets.remove(original + 1);

                            for offset in &mut state.offsets[original + 1..] {
                                *offset -= height;
                            }

                            // Only drop the removed item's own tree; other
                            // visible items keep theirs (and thus keep any
                            // state cached inside, e.g. a `lazy` cache) so
                            // a removal doesn't force unrelated rows to
                            // rebuild.
                            let removed = state
                                .visible_layouts
                                .iter()
                                .position(|(i, _, _)| *i == original)
                                .map(|removed_index| {
                                    let _ = state
                                        .visible_layouts
                                        .remove(removed_index);
                                })
                                .is_some();

                            let mut shifted = false;

                            for (i, layout, _) in &mut state.visible_layouts {
                                if *i > original {
                                    *i -= 1;
                                    *layout = layout
                                        .move_to((0.0, state.offsets[*i]));
                                    shifted = true;
                                }
                            }

                            state.visible_outdated |= removed || shifted;

                            state.size.height -= height;

                            if original_width == state.size.width {
                                state.size.width = state.widths.iter().fold(
                                    0.0,
                                    |current, candidate| {
                                        current.max(*candidate)
                                    },
                                );
                            }

                            // Removing the last row promotes the previous
                            // row, so queue an update to re-measure it.
                            if original >= 1 && original == self.content.len() {
                                changes.push_front(Change::Updated {
                                    original: original - 1,
                                    current: original - 1,
                                });
                            }
                        }
                        Change::Inserted { original, current } => {
                            let (key, value) = self
                                .content
                                .items
                                .get_index(current)
                                .expect("Indexing should work");
                            let mut new_element = (self.view_item)(
                                current,
                                key.clone(),
                                value.clone(),
                            );

                            let mut new_tree = Tree::new(&new_element);
                            new_element.diff(&mut new_tree);

                            new_element.layout(
                                &mut new_tree,
                                renderer,
                                &state.last_limits,
                            );

                            let new_layout = Layout::new(new_tree.size);
                            let size = new_layout.size();
                            let insertion_y = state.offsets[original];

                            state.widths.insert(original, size.width);
                            state.offsets.insert(
                                original + 1,
                                insertion_y + size.height,
                            );

                            for offset in &mut state.offsets[original + 2..] {
                                *offset += size.height;
                            }

                            // An insertion touching (or landing inside) the
                            // currently tracked visible range needs its own
                            // tree spliced in now to keep that range
                            // contiguous; one landing outside it is picked
                            // up later by the usual prepend/append-on-scroll
                            // logic. Either way, other visible items just
                            // get reindexed/repositioned in place, keeping
                            // their trees (and any cached state, e.g. a
                            // `lazy` cache).
                            let touches_tracked_range = state
                                .visible_layouts
                                .first()
                                .zip(state.visible_layouts.last())
                                .is_some_and(|((first, ..), (last, ..))| {
                                    original >= *first && original <= *last + 1
                                });

                            for (i, layout, _) in &mut state.visible_layouts {
                                if *i >= original {
                                    *i += 1;
                                    *layout = layout
                                        .move_to((0.0, state.offsets[*i]));
                                }
                            }

                            if touches_tracked_range {
                                let insert_at = state
                                    .visible_layouts
                                    .iter()
                                    .position(|(i, _, _)| *i > original)
                                    .unwrap_or(state.visible_layouts.len());

                                state.visible_layouts.insert(
                                    insert_at,
                                    (
                                        original,
                                        new_layout.move_to((0.0, insertion_y)),
                                        new_tree,
                                    ),
                                );
                            }

                            state.visible_outdated = true;

                            state.size.width = state.size.width.max(size.width);
                            state.size.height += size.height;

                            // Appending a row changes who is last, so queue
                            // an update to re-measure the previous row.
                            if current >= 1 && current + 1 == self.content.len() {
                                changes.push_front(Change::Updated {
                                    original: current - 1,
                                    current: current - 1,
                                });
                            }
                        }
                    }
                }
            }
            Task::Computing { .. } => {
                if !changes.is_empty() {
                    changes.clear();
                    state.recompute(self.content.len());

                    // The items have already been mutated, so the partially
                    // measured offsets behind the running batches no longer
                    // line up: the measurement has to start over. Starting
                    // over batched (a batch per relayout) can livelock in
                    // long lists — ambient content updates (new messages,
                    // read receipts, media finishing loading, ...) arriving
                    // mid-measure restart it from zero forever. Finish the
                    // restarted measurement in this layout pass instead:
                    // one heavy frame beats a livelock.
                    finish_restarted_measurement = true;
                }
            }
        }

        // Recompute if new
        {
            let mut is_new = self.content.is_new.borrow_mut();

            if *is_new {
                state.recompute(self.content.len());
                *is_new = false;
            }
        }

        loop {
            let Task::Computing {
                current,
                size,
                widths,
                offsets,
            } = &mut state.task
            else {
                break;
            };

            const MAX_BATCH_SIZE: usize = 50;

            let end = (*current + MAX_BATCH_SIZE).min(self.content.len());

            let batch = &self.content.items[*current..end];

            let mut max_width = size.width;
            let mut accumulated_height = offsets.last().copied().unwrap_or(0.0);

            for (i, (key, value)) in batch.iter().enumerate() {
                let mut element =
                    (self.view_item)(*current + i, key.clone(), value.clone());
                let mut tree = Tree::new(&element);
                element.diff(&mut tree);

                element.layout(&mut tree, renderer, &state.last_limits);
                let layout =
                    Layout::new(tree.size).move_to((0.0, accumulated_height));

                let bounds = layout.bounds();

                max_width = max_width.max(bounds.width);
                accumulated_height += bounds.height;

                offsets.push(accumulated_height);
                widths.push(bounds.width);
            }

            *size = Size::new(max_width, accumulated_height);

            if end < self.content.len() {
                *current = end;
            } else {
                state.offsets = std::mem::take(offsets);
                state.widths = std::mem::take(widths);
                state.size = std::mem::take(size);
                state.task = Task::Idle;
                // The offsets changed wholesale after a full re-measure,
                // so visible rows must be rebuilt against the new ones.
                state.visible_outdated = true;
            }

            // Normally a single batch per relayout keeps each frame light;
            // only a measurement restarted by content changes must be
            // finished here, see the `Task::Computing` arm above.
            if !finish_restarted_measurement {
                break;
            }
        }

        // Always report the last complete size, never the partial one
        // from an in-flight `Task::Computing`.
        let current_size = state.size;

        let intrinsic_size = Size::new(
            current_size.width,
            current_size.height
                + self.content.len().saturating_sub(1) as f32 * self.spacing,
        );

        let size =
            limits.resolve(Length::Shrink, Length::Shrink, intrinsic_size);

        tree.size = size;
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
        let state = tree.state.downcast_mut::<State>();
        let offset = layout.position() - Point::ORIGIN;

        for (element, (index, layout, tree)) in self
            .visible_elements
            .iter_mut()
            .zip(&mut state.visible_layouts)
        {
            element.update(
                tree,
                event,
                layout.move_to(
                    layout.position()
                        + offset
                        + Vector::new(0.0, self.spacing * *index as f32),
                ),
                cursor,
                renderer,
                shell,
                viewport,
            );
        }

        let is_redraw =
            matches!(event, Event::Window(window::Event::RedrawRequested(_)));

        let rows_desynced = state.visible_outdated
            || self.visible_elements.len() != state.visible_layouts.len();

        if is_redraw || rows_desynced {
            if is_redraw {
                if matches!(state.task, Task::Computing { .. }) {
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
            } else {
                shell.invalidate_layout();
                shell.request_redraw();
            }

            // The offsets are stale while a re-measure is in flight, so
            // wait for it to settle before materializing rows.
            if matches!(state.task, Task::Computing { .. }) {
                return;
            }

            let offsets = &state.offsets;

            let start =
                match binary_search_with_index_by(offsets, |i, height| {
                    (*height + i.saturating_sub(1) as f32 * self.spacing)
                        .partial_cmp(&(viewport.y - offset.y))
                        .unwrap_or(Ordering::Equal)
                }) {
                    Ok(i) => i,
                    Err(i) => i.saturating_sub(1),
                }
                .min(self.content.len());

            let end = match binary_search_with_index_by(offsets, |i, height| {
                (*height + i.saturating_sub(1) as f32 * self.spacing)
                    .partial_cmp(&(viewport.y + viewport.height - offset.y))
                    .unwrap_or(Ordering::Equal)
            }) {
                Ok(i) => i,
                Err(i) => i,
            }
            .min(self.content.len());

            if state.visible_outdated
                || state.visible_layouts.len() != self.visible_elements.len()
            {
                self.visible_elements.clear();
                state.visible_outdated = false;
            }

            // If view was recreated, we repopulate the visible elements
            // out of the internal visible layouts
            if self.visible_elements.is_empty() {
                self.visible_elements = state
                    .visible_layouts
                    .iter()
                    .map(|(i, _, _)| {
                        let (key, value) = self
                            .content
                            .items
                            .get_index(*i)
                            .expect("Indexing should work");
                        (self.view_item)(*i, key.clone(), value.clone())
                    })
                    .collect();
            }

            // Clear no longer visible elements
            let top = state
                .visible_layouts
                .iter()
                .take_while(|(i, _, _)| *i < start)
                .count();

            let bottom = state
                .visible_layouts
                .iter()
                .rev()
                .take_while(|(i, _, _)| *i >= end)
                .count();

            let _ = self.visible_elements.splice(..top, []);
            let _ = state.visible_layouts.splice(..top, []);

            let _ = self
                .visible_elements
                .splice(self.visible_elements.len() - bottom.., []);
            let _ = state
                .visible_layouts
                .splice(state.visible_layouts.len() - bottom.., []);

            // Prepend new visible elements
            if let Some(first_visible) =
                state.visible_layouts.first().map(|(i, _, _)| *i)
                && start < first_visible
            {
                for (i, (key, value)) in
                    self.content.items[start..first_visible].iter().enumerate()
                {
                    let mut element =
                        (self.view_item)(start + i, key.clone(), value.clone());
                    let mut tree = Tree::new(&element);
                    element.diff(&mut tree);

                    element.layout(&mut tree, renderer, &state.last_limits);
                    let layout =
                        Layout::new(tree.size).move_to((0.0, offsets[start + i]));

                    state.visible_layouts.insert(i, (start + i, layout, tree));
                    self.visible_elements.insert(i, element);
                }
            }

            // Append new visible elements
            let last_visible = state
                .visible_layouts
                .last()
                .map(|(i, _, _)| *i + 1)
                .unwrap_or(start);

            if last_visible < end {
                for (i, (key, value)) in
                    self.content.items[last_visible..end].iter().enumerate()
                {
                    let mut element = (self.view_item)(
                        last_visible + i,
                        key.clone(),
                        value.clone(),
                    );
                    let mut tree = Tree::new(&element);
                    element.diff(&mut tree);

                    element.layout(&mut tree, renderer, &state.last_limits);
                    let layout =
                        Layout::new(tree.size).move_to((0.0, offsets[last_visible + i]));

                    state.visible_layouts.push((
                        last_visible + i,
                        layout,
                        tree,
                    ));
                    self.visible_elements.push(element);
                }
            }

            for (element, (index, item_layout, tree)) in self
                .visible_elements
                .iter_mut()
                .zip(&mut state.visible_layouts)
            {
                element.update(
                    tree,
                    event,
                    item_layout.move_to(
                        item_layout.position()
                            + offset
                            + Vector::new(0.0, self.spacing * *index as f32),
                    ),
                    cursor,
                    renderer,
                    shell,
                    viewport,
                );
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let offset = layout.position() - Point::ORIGIN;

        for (element, (index, layout, tree)) in
            self.visible_elements.iter().zip(&state.visible_layouts)
        {
            element.draw(
                tree,
                renderer,
                theme,
                style,
                layout.move_to(
                    layout.position()
                        + offset
                        + Vector::new(0.0, self.spacing * *index as f32),
                ),
                cursor,
                viewport,
            );
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
        let state = tree.state.downcast_ref::<State>();
        let offset = layout.position() - Point::ORIGIN;

        self.visible_elements
            .iter()
            .zip(&state.visible_layouts)
            .map(|(element, (index, layout, tree))| {
                element.mouse_interaction(
                    tree,
                    layout.move_to(
                        layout.position()
                            + offset
                            + Vector::new(0.0, self.spacing * *index as f32),
                    ),
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();

        state.spacing = self.spacing;
        operation.custom(self.id.as_ref(), layout.bounds(), state);

        let offset = layout.position() - Point::ORIGIN;
        for (element, (index, layout, tree)) in self
            .visible_elements
            .iter_mut()
            .zip(&mut state.visible_layouts)
        {
            element.operate(
                tree,
                layout.move_to(
                    layout.position()
                        + offset
                        + Vector::new(0.0, self.spacing * *index as f32),
                ),
                viewport,
                renderer,
                operation,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        let offset = layout.position() - Point::ORIGIN;

        self.visible_elements
            .iter_mut()
            .zip(&mut state.visible_layouts)
            .flat_map(|(child, (index, layout, tree))| {
                child.overlay(
                    tree,
                    layout.move_to(
                        layout.position()
                            + offset
                            + Vector::new(0.0, self.spacing * *index as f32),
                    ),
                    renderer,
                    _viewport,
                    translation,
                    window,
                )
            })
            .collect()
    }
}

impl<'a, S: Eq + Hash + Clone + Debug + 'a, T, Message, Theme, Renderer>
    From<List<'a, S, T, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    T: 'a,
    Message: 'a,
    Theme: 'a,
    Renderer: core::Renderer + 'a,
{
    fn from(list: List<'a, S, T, Message, Theme, Renderer>) -> Self {
        list._boxed()
    }
}

use std::hash::Hash;

/// The backing data for a [`List`].
///
/// Holds the full item vector and tracks mutations (push, remove,
/// update) so the widget can reconcile its tree incrementally
/// instead of rebuilding from scratch every frame.
#[derive(Debug, PartialEq, Eq)]
pub struct Content<S: Eq + Hash + Clone, T> {
    items: Rc<IndexMap<S, Rc<T>>>,
    is_new: Rc<RefCell<bool>>,
    changes: Rc<RefCell<VecDeque<Change>>>,
}

impl<S: Eq + Hash + Clone, T> Clone for Content<S, T> {
    fn clone(&self) -> Self {
        Self {
            items: Rc::clone(&self.items),
            is_new: Rc::clone(&self.is_new),
            changes: Rc::clone(&self.changes),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Updated { original: usize, current: usize },
    Removed { original: usize, current: usize },
    Inserted { original: usize, current: usize },
}

impl<S: Eq + Hash + Clone + Debug, T> Content<S, T> {
    /// Creates an empty [`Content`].
    pub fn new() -> Self {
        Self {
            items: Rc::new(IndexMap::new()),
            is_new: Rc::new(RefCell::new(true)),
            changes: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    /// Creates a [`Content`] pre-populated with `items`.
    pub fn with_items(items: IndexMap<S, T>) -> Self {
        let map = items.into_iter().map(|(s, t)| (s, Rc::new(t))).collect();
        Self {
            items: Rc::new(map),
            is_new: Rc::new(RefCell::new(true)),
            changes: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    /// Returns a reference to the item at the given key, if it exists.
    pub fn get(&self, key: &S) -> Option<&T>
    where
        T: Clone,
    {
        self.items.get(key).map(|item| (*item).borrow())
    }

    /// Returns a reference to the item at `index`, if it exists.
    pub fn get_index(&self, index: usize) -> Option<&T> {
        self.items.get_index(index).map(|item| (*item.1).borrow())
    }

    /// Returns a reference to the item at the given key, if it exists.
    pub fn get_mut(&mut self, key: &S) -> Option<&mut T>
    where
        T: Clone,
    {
        let index = self.items.get_index_of(key)?;

        self.changes.borrow_mut().push_back(Change::Updated {
            original: index,
            current: index,
        });
        Rc::make_mut(&mut self.items).get_mut(key).map(Rc::make_mut)
    }

    /// Returns a mutable reference to the item at `index`, recording
    /// the mutation so the widget tree stays in sync.
    pub fn get_index_mut(&mut self, index: usize) -> Option<&mut T>
    where
        T: Clone,
    {
        if index >= self.items.len() {
            // Don't record a `Change` for a nonexistent item: replaying
            // it would index the widget's internal offsets/widths out of
            // bounds.
            return None;
        }

        self.changes.borrow_mut().push_back(Change::Updated {
            original: index,
            current: index,
        });
        Rc::make_mut(&mut self.items)
            .get_index_mut(index)
            .map(|(_, item)| Rc::make_mut(item))
    }

    /// Appends an item to the end of the list.
    pub fn push(&mut self, key: S, item: T)
    where
        T: Clone,
    {
        self.insert(self.items.len(), key, item);
    }

    /// Inserts an item at `index`, shifting later items (and their
    /// tracked identity, so their widget state survives) up by one
    /// position.
    pub fn insert(&mut self, index: usize, key: S, item: T)
    where
        T: Clone,
    {
        // `IndexMap::insert_before` panics when `index > len`; clamp so an
        // out-of-range index appends instead.
        let index = index.min(self.items.len());
        let mut changes = self.changes.borrow_mut();

        // Update pending changes after insertion
        for change in changes.iter_mut() {
            match change {
                Change::Updated { current, .. }
                | Change::Removed { current, .. }
                | Change::Inserted { current, .. }
                    if *current >= index =>
                {
                    // Increment index of later changes
                    *current += 1;
                }
                _ => {}
            }
        }

        changes.push_back(Change::Inserted {
            original: index,
            current: index,
        });

        Rc::make_mut(&mut self.items).insert_before(index, key, Rc::new(item));
    }

    /// Removes and returns the item at `index`, updating pending
    /// change indices accordingly.
    pub fn remove(&mut self, index: usize) -> Option<Rc<T>>
    where
        T: Clone,
    {
        if index >= self.items.len() {
            // Don't record a `Change` for a nonexistent item: replaying
            // it would index the widget's internal offsets/widths out of
            // bounds.
            return None;
        }

        let mut changes = self.changes.borrow_mut();
        changes.retain_mut(|change| match change {
            // A pending update/insert for the exact item being removed is
            // now moot -- drop it so it isn't replayed against whatever
            // item shifts into this slot.
            Change::Updated { current, .. }
            | Change::Inserted { current, .. }
                if *current == index =>
            {
                false
            }
            Change::Updated { current, .. }
            | Change::Removed { current, .. }
            | Change::Inserted { current, .. }
                if *current > index =>
            {
                // Decrement index of later changes
                *current -= 1;

                true
            }
            _ => true,
        });

        changes.push_back(Change::Removed {
            original: index,
            current: index,
        });

        Rc::make_mut(&mut self.items)
            .shift_remove_index(index)
            .map(|item| item.1)
    }

    /// Returns `true` if the list contains no items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the number of items in the list.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns a reference to the underlying indexmap
    pub fn index_map(&self) -> Rc<IndexMap<S, Rc<T>>> {
        self.items.clone()
    }

    /// Consumes the [`Content`] and returns the underlying [`Vec<T>`].
    pub fn into_index_map(self) -> IndexMap<S, Rc<T>>
    where
        T: Clone,
    {
        Rc::try_unwrap(self.items).unwrap_or_else(|rc| (*rc).clone())
    }
}

impl<S: Eq + Hash + Clone + Debug, T> Default for Content<S, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Eq + Hash + Clone + Debug, T> FromIterator<(S, T)> for Content<S, T> {
    fn from_iter<I: IntoIterator<Item = (S, T)>>(iter: I) -> Self {
        Self::with_items(iter.into_iter().collect())
    }
}

/// SAFETY: Copied from the `std` library.
#[allow(unsafe_code)]
fn binary_search_with_index_by<'a, T, F>(
    slice: &'a [T],
    mut f: F,
) -> Result<usize, usize>
where
    F: FnMut(usize, &'a T) -> Ordering,
{
    use std::cmp::Ordering::*;

    // INVARIANTS:
    // - 0 <= left <= left + size = right <= self.len()
    // - f returns Less for everything in self[..left]
    // - f returns Greater for everything in self[right..]
    let mut size = slice.len();
    let mut left = 0;
    let mut right = size;
    while left < right {
        let mid = left + size / 2;

        // SAFETY: the while condition means `size` is strictly positive, so
        // `size/2 < size`. Thus `left + size/2 < left + size`, which
        // coupled with the `left + size <= self.len()` invariant means
        // we have `left + size/2 < self.len()`, and this is in-bounds.
        let cmp = f(mid, unsafe { slice.get_unchecked(mid) });

        // This control flow produces conditional moves, which results in
        // fewer branches and instructions than if/else or matching on
        // cmp::Ordering.
        // This is x86 asm for u8: https://rust.godbolt.org/z/698eYffTx.
        left = if cmp == Less { mid + 1 } else { left };
        right = if cmp == Greater { mid } else { right };
        if cmp == Equal {
            return Ok(mid);
        }

        size = right - left;
    }

    Err(left)
}

use std::any::Any;

pub struct FindItemBounds {
    pub target_id: Option<Id>,
    pub target_index: usize,
    pub result: Option<Rectangle>,
}

impl FindItemBounds {
    pub fn new(target_id: Option<Id>, target_index: usize) -> Self {
        Self {
            target_id,
            target_index,
            result: None,
        }
    }
}

impl Operation<Option<Rectangle>> for FindItemBounds {
    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn Operation<Option<Rectangle>>),
    ) {
        operate(self);
    }

    fn custom(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut (dyn Any + 'static),
    ) {
        if self.target_id.is_some() && self.target_id.as_ref() != id {
            return;
        }

        if let Some(list_state) = state.downcast_ref::<State>() {
            let index = self.target_index;

            if index < list_state.offsets.len().saturating_sub(1) {
                let start_y = list_state.offsets[index];
                let end_y = list_state.offsets[index + 1];
                let height = end_y - start_y;
                let width =
                    list_state.widths.get(index).copied().unwrap_or(0.0);

                // let top_offset = start_y + (index as f32 * list_state.spacing);

                // Calculate y relative to the bottom (or opposite end) of bounds
                let item_y = bounds.height - end_y;

                self.result = Some(Rectangle {
                    x: bounds.x,
                    y: item_y,
                    width,
                    height,
                });
            }
        }
    }
    fn finish(&self) -> Outcome<Option<Rectangle>> {
        // Return the captured bounds as the operation outcome
        Outcome::Some(self.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Never, Theme, shell};
    use iced_widget::Widget as _;

    // A vertical `Scrollable` lays its content out with an unbounded
    // height (and a width it shrinks by the scrollbar whenever the
    // scrollbar is showing).
    const LIMITS: layout::Limits = layout::Limits::new(
        Size::ZERO,
        Size {
            width: 1024.0,
            height: f32::INFINITY,
        },
    );

    fn fixed_item(
        _index: usize,
        _key: u8,
        _item: Rc<u8>,
    ) -> Element<'static, Never, Theme, ()> {
        iced_widget::Space::new().width(100).height(20).boxed()
    }

    fn content_of(len: u8) -> Content<u8, u8> {
        Content::with_items((0..len).map(|i| (i, i)).collect())
    }

    fn new_list(
        content: Content<u8, u8>,
    ) -> (Element<'static, Never, Theme, ()>, Tree) {
        let mut element: Element<'static, Never, Theme, ()> =
            List::new(content, fixed_item).into();
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);

        (element, tree)
    }

    fn state_of(tree: &Tree) -> &State {
        tree.state.downcast_ref::<State>()
    }

    #[test]
    fn compute_finishes_in_batches_and_reports_complete_size() {
        // 120 items = 3 batches of MAX_BATCH_SIZE (50), so the first two
        // `layout` calls leave the `List` mid-compute.
        let (mut element, mut tree) = new_list(content_of(120));

        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(
            state_of(&tree).task,
            Task::Computing { current: 50, .. }
        ));
        assert_eq!(tree.size, Size::ZERO, "no complete size exists yet");

        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(
            state_of(&tree).task,
            Task::Computing { current: 100, .. }
        ));

        // Third pass finishes the compute; the complete size is reported.
        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::new(100.0, 120.0 * 20.0));
    }

    #[test]
    fn layout_reports_last_complete_size_while_computing() {
        let (mut element, mut tree) = new_list(content_of(120));

        // Finish the initial compute.
        for _ in 0..3 {
            element.layout(&mut tree, &(), &LIMITS);
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::new(100.0, 120.0 * 20.0));

        // A change in limits (e.g. a `Scrollable` reserving scrollbar
        // width) restarts the compute. While it runs, the reported size
        // must stay pinned to the last complete size instead of dropping
        // to the partial batch size: a reported size that shrinks
        // mid-compute is exactly what makes a scrollbar-reserving parent
        // flip visibility, change the limits back, and loop forever.
        let narrower = layout::Limits::new(
            Size::ZERO,
            Size {
                width: 500.0,
                height: f32::INFINITY,
            },
        );

        element.layout(&mut tree, &(), &narrower);
        assert!(matches!(
            state_of(&tree).task,
            Task::Computing { current: 50, .. }
        ));
        assert_eq!(
            tree.size,
            Size::new(100.0, 120.0 * 20.0),
            "mid-compute layout must report the last complete size, not \
             the partial batch size"
        );
    }

    #[test]
    fn viewport_height_change_does_not_restart_compute() {
        // The chat timeline lives in a vertical `Scrollable`, which lays
        // out its content with an unbounded height axis. A height-only
        // limits change — the input next to the timeline taking another
        // line and shrinking the scrollable's viewport — must not restart
        // the batched measurement: every restart re-layouts the whole
        // application once per batch, which made typing in the input lag
        // for seconds and flood the runtime with invalidation warnings.
        let tall = layout::Limits::with_flags(
            Size::ZERO,
            Size {
                width: 1024.0,
                height: 800.0,
            },
            Size::new(false, false),
            Size::new(false, true),
        );
        let shrunk = layout::Limits::with_flags(
            Size::ZERO,
            Size {
                width: 1024.0,
                height: 600.0,
            },
            Size::new(false, false),
            Size::new(false, true),
        );

        let (mut element, mut tree) = new_list(content_of(120));

        element.layout(&mut tree, &(), &tall);
        assert!(matches!(
            state_of(&tree).task,
            Task::Computing { current: 50, .. }
        ));

        // A height-only change continues the running compute instead of
        // starting over.
        element.layout(&mut tree, &(), &shrunk);
        assert!(matches!(
            state_of(&tree).task,
            Task::Computing { current: 100, .. }
        ));

        element.layout(&mut tree, &(), &shrunk);
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::new(100.0, 120.0 * 20.0));

        // A width change still restarts the measurement: item wrapping
        // depends on it.
        let narrower = layout::Limits::with_flags(
            Size::ZERO,
            Size {
                width: 500.0,
                height: 600.0,
            },
            Size::new(false, false),
            Size::new(false, true),
        );

        element.layout(&mut tree, &(), &narrower);
        assert!(matches!(
            state_of(&tree).task,
            Task::Computing { current: 50, .. }
        ));
    }

    #[test]
    fn changes_arriving_mid_compute_finish_the_restarted_measurement() {
        // Ambient content updates (timeline diffs, read receipts, media
        // finishing loading) used to restart an in-flight measurement
        // from zero; changes arriving faster than the batched compute
        // completed livelocked long lists in a relayout-per-frame. A
        // restart must finish synchronously instead.
        let content = content_of(120);
        let mut shared = content.clone();
        let (mut element, mut tree) = new_list(content);

        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(
            state_of(&tree).task,
            Task::Computing { current: 50, .. }
        ));

        // An update arrives while the measurement is in flight, and —
        // like the timeline rebuilding its view on a diff — the app
        // lays out a fresh element carrying the new content.
        shared.push(120, 120);
        let mut fresh: Element<'static, Never, Theme, ()> =
            List::new(shared, fixed_item).into();

        Widget::layout(&mut fresh, &mut tree, &(), &LIMITS);
        assert!(
            matches!(state_of(&tree).task, Task::Idle),
            "a restarted measurement must finish in a single layout pass"
        );
        assert_eq!(tree.size, Size::new(100.0, 121.0 * 20.0));
    }

    #[test]
    fn compute_finishes_when_scrollbar_flips_visibility() {
        // Reproduces the layout loop: 150 fixed-size items = 3 compute
        // batches of MAX_BATCH_SIZE (50). In a viewport (2500px) that is
        // taller than two batches (2000px) but shorter than the full
        // content (3000px), the *partial* sizes straddle the viewport
        // height. A `Scrollable` that reserves scrollbar width then
        // flips the scrollbar on and off as the partial size crosses the
        // viewport, and every flip changes the limits we are laid out
        // with, restarting the compute from the first batch. The compute
        // can then never finish and layout runs forever.
        let (list, _) = new_list(content_of(150));

        let mut scrollable: Element<'static, Never, Theme, ()> =
            iced_widget::Scrollable::new(list)
                // `spacing` opts the scrollable into reserving scrollbar
                // width (and re-laying out on visibility flips).
                .spacing(1.0)
                .width(Length::Fill)
                .height(Length::Fill)
                .boxed();
        let mut tree = Tree::new(&scrollable);
        scrollable.diff(&mut tree);

        let viewport = layout::Limits::new(
            Size::ZERO,
            Size {
                width: 300.0,
                height: 2500.0,
            },
        );

        let mut passes = 0;
        loop {
            scrollable.layout(&mut tree, &(), &viewport);
            passes += 1;

            assert!(
                passes <= 32,
                "list compute never finished: the scrollable keeps \
                 flipping the scrollbar (and our limits), restarting the \
                 compute on every layout pass"
            );

            if matches!(
                tree.children[0].state.downcast_ref::<State>().task,
                Task::Idle
            ) {
                break;
            }
        }

        // The scrollbar must actually have appeared (narrowing the limits
        // we compute against) for this test to exercise the flip path.
        let list_state = tree.children[0].state.downcast_ref::<State>();
        assert!(
            list_state.last_limits.max.width < 300.0,
            "expected the scrollbar to flip visible at least once and \
             shrink the layout width"
        );
        assert_eq!(tree.children[0].size, Size::new(100.0, 150.0 * 20.0));
    }

    #[test]
    fn out_of_range_mutations_are_inert() {
        let content = content_of(5);
        let mut shared = content.clone();
        let (mut element, mut tree) = new_list(content);

        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(state_of(&tree).task, Task::Idle));

        // Indices past the end (e.g. stale app state after a removal)
        // must be no-ops, not queued `Change`s that later panic the
        // widget's replay logic.
        assert!(shared.remove(7).is_none());
        assert!(shared.get_index_mut(7).is_none());

        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::new(100.0, 5.0 * 20.0));
    }

    #[test]
    fn removals_from_empty_content_are_inert() {
        let content = content_of(0);
        let mut shared = content.clone();
        let (mut element, mut tree) = new_list(content);

        assert!(shared.remove(0).is_none());
        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::ZERO);
    }

    #[test]
    fn insert_beyond_len_appends() {
        let content = content_of(5);
        let mut shared = content.clone();
        let (mut element, mut tree) = new_list(content);

        element.layout(&mut tree, &(), &LIMITS);

        // Mutate only once no other clone keeps the `Rc` shared
        // (`Rc::make_mut` would otherwise fork the item map).
        drop(element);
        shared.insert(100, 100, 100);

        let (mut element, mut tree) = new_list(shared.clone());
        element.layout(&mut tree, &(), &LIMITS);

        assert_eq!(tree.size, Size::new(100.0, 6.0 * 20.0));
        assert_eq!(shared.len(), 6);
        assert!(shared.get_index(5).is_some());
    }

    #[test]
    fn mutation_while_element_alive_heals_instead_of_panicking() {
        // If a mutation happens while the list element still holds a
        // clone, `Rc::make_mut` forks the item map and the element's
        // snapshot goes stale. Replaying the queued change against the
        // snapshot must not panic; it should just reset the layout.
        let content = content_of(5);
        let mut shared = content.clone();
        let (mut element, mut tree) = new_list(content);

        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(state_of(&tree).task, Task::Idle));

        shared.push(5, 5);

        element.layout(&mut tree, &(), &LIMITS);
        // The element's snapshot stayed stale (its `Rc` forked), but the
        // queued change was discarded instead of panicking the replay.
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::new(100.0, 5.0 * 20.0));
    }

    #[test]
    fn stale_rows_reconcile_on_any_event() {
        // A viewport that shows every row.
        let viewport = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 400.0,
        };

        let content = content_of(5);
        let mut shared = content.clone();

        let mut list = List::new(content, fixed_item);
        let mut tree =
            Tree::new(&Element::from(List::new(shared.clone(), fixed_item)));

        Widget::layout(&mut list, &mut tree, &(), &LIMITS);

        let headless = window::Headless;
        let mut bus: shell::Bus<Never> = shell::Bus::new();
        let cursor = mouse::Cursor::Unavailable;
        let layout = Layout::new(viewport.size());

        // A redraw event populates the visible rows.
        let redraw = Event::Window(window::Event::RedrawRequested(
            std::time::Instant::now(),
        ));
        let mut shell = Shell::new(&headless, shell::Waker::noop(), &mut bus);
        Widget::update(
            &mut list,
            &mut tree,
            &redraw,
            layout,
            cursor,
            &(),
            &mut shell,
            &viewport,
        );
        assert_eq!(list.visible_elements.len(), 5);

        // Simulate a background change and a recreated view (the tree,
        // with its visible layouts, persists): the rows are desynced
        // after `layout`, before a redraw was ever requested.
        drop(list);
        let _ = shared.get_index_mut(2);

        let mut list = List::new(shared.clone(), fixed_item);
        Widget::layout(&mut list, &mut tree, &(), &LIMITS);
        assert!(state_of(&tree).visible_outdated);
        assert_eq!(list.visible_elements.len(), 0);

        // A non-draw event must reconcile the rows instead of waiting
        // for a `RedrawRequested` event (i.e. for user input).
        let moved = Event::Mouse(mouse::Event::CursorMoved {
            position: Point::ORIGIN,
        });
        let mut shell = Shell::new(&headless, shell::Waker::noop(), &mut bus);
        Widget::update(
            &mut list,
            &mut tree,
            &moved,
            layout,
            cursor,
            &(),
            &mut shell,
            &viewport,
        );

        assert_eq!(list.visible_elements.len(), 5);
        assert!(!state_of(&tree).visible_outdated);
    }

    // A row whose height depends on the available width, 20px wide and
    // 60px narrow, like wrapped text at a different window size.
    struct WidthSensitiveRow;

    impl iced_core::widget::Meta for WidthSensitiveRow {}

    impl Widget<Never, Theme, ()> for WidthSensitiveRow {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<()>()
        }
        fn state(&self) -> tree::State {
            tree::State::new(())
        }
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Shrink)
        }
        fn layout(&mut self, tree: &mut Tree, _renderer: &(), limits: &layout::Limits) {
            let height = if limits.max.width > 600.0 { 20.0 } else { 60.0 };
            tree.size = Size::new(limits.max.width.min(1000.0), height);
        }
        fn update(
            &mut self,
            _tree: &mut Tree,
            _event: &Event,
            _layout: Layout,
            _cursor: mouse::Cursor,
            _renderer: &(),
            _shell: &mut Shell<'_, Never>,
            _viewport: &Rectangle,
        ) {
        }
        fn draw(
            &self,
            _tree: &Tree,
            _renderer: &mut (),
            _theme: &Theme,
            _style: &renderer::Style,
            _layout: Layout,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
        ) {
        }
    }

    impl<'a> From<WidthSensitiveRow> for Element<'a, Never, Theme, ()> {
        fn from(row: WidthSensitiveRow) -> Self {
            row.boxed()
        }
    }

    #[test]
    fn width_change_rematerializes_visible_rows_at_new_offsets() {
        // Regression where rows materialized during a batched re-measure
        // after a large resize never moved to their new offsets.
        const SPACING: f32 = 8.0;
        let view = |_index: usize, _key: u8, _item: Rc<u8>| -> Element<'static, Never, Theme, ()> {
            WidthSensitiveRow.into()
        };

        let wide = layout::Limits::new(
            Size::ZERO,
            Size {
                width: 800.0,
                height: f32::INFINITY,
            },
        );
        let narrow = layout::Limits::new(
            Size::ZERO,
            Size {
                width: 400.0,
                height: f32::INFINITY,
            },
        );
        let viewport = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 500.0,
        };

        let content = Content::with_items((0..100u8).map(|i| (i, 0u8)).collect());
        let mut shared = content.clone();
        let mut element: Element<'static, Never, Theme, ()> =
            List::new(content, view).spacing(SPACING).into();
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);

        // Settle at the wide width and materialize the visible rows.
        for _ in 0..10 {
            element.layout(&mut tree, &(), &wide);
            let redraw = Event::Window(window::Event::RedrawRequested(
                std::time::Instant::now(),
            ));
            let mut bus: shell::Bus<Never> = shell::Bus::new();
            let mut shell = Shell::new(&window::Headless, shell::Waker::noop(), &mut bus);
            let frame_layout = Layout::new(tree.size);
            element.update(
                &mut tree,
                &redraw,
                frame_layout,
                mouse::Cursor::Unavailable,
                &(),
                &mut shell,
                &viewport,
            );
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size.height, 100.0 * 20.0 + 99.0 * SPACING);
        assert!(!state_of(&tree).visible_layouts.is_empty());

        // Large resize, running a layout pass plus redraw per frame
        // until the re-measure finishes.
        drop(element);
        for _ in 0..30 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), view).spacing(SPACING).into();
            element.layout(&mut tree, &(), &narrow);
            let redraw = Event::Window(window::Event::RedrawRequested(
                std::time::Instant::now(),
            ));
            let mut bus: shell::Bus<Never> = shell::Bus::new();
            let mut shell = Shell::new(&window::Headless, shell::Waker::noop(), &mut bus);
            let frame_layout = Layout::new(tree.size);
            element.update(
                &mut tree,
                &redraw,
                frame_layout,
                mouse::Cursor::Unavailable,
                &(),
                &mut shell,
                &viewport,
            );
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size.height, 100.0 * 60.0 + 99.0 * SPACING);

        // Every visible row must sit at its final offset with its new
        // height, contiguous with its neighbour.
        let state = state_of(&tree);
        let mut rows: Vec<(usize, Rectangle)> = state
            .visible_layouts
            .iter()
            .map(|(i, layout, _)| (*i, layout.bounds()))
            .collect();
        rows.sort_by_key(|(i, _)| *i);
        assert!(!rows.is_empty());
        for (i, rect) in &rows {
            assert_eq!(rect.height, 60.0, "row {i} height after resize");
            assert_eq!(rect.y, state.offsets[*i], "row {i} y after resize");
        }
        for w in rows.windows(2) {
            assert!(
                (w[1].1.y - (w[0].1.y + w[0].1.height)).abs() < 0.01,
                "rows {} and {} not contiguous",
                w[0].0,
                w[1].0
            );
        }
    }

    #[test]
    fn update_of_last_item_is_folded_into_total_height() {
        // Regression where updating the last item skipped folding its
        // new height into the total, hiding the row's bottom on scroll.
        let height_item =
            |_index: usize, _key: u8, item: Rc<u8>| -> Element<'static, Never, Theme, ()> {
                iced_widget::Space::new()
                    .width(100)
                    .height(*item as f32 * 20.0)
                    .boxed()
            };

        let content = Content::with_items((0..5u8).map(|i| (i, 1u8)).collect());
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, height_item).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            element.layout(&mut tree, &(), &LIMITS);
            assert!(matches!(state_of(&tree).task, Task::Idle));
            assert_eq!(tree.size, Size::new(100.0, 5.0 * 20.0));
            tree
        };

        // Grow the last item, e.g. media in the newest row loaded.
        *shared.get_index_mut(4).unwrap() = 2;

        let mut element: Element<'static, Never, Theme, ()> =
            List::new(shared.clone(), height_item).into();
        element.layout(&mut tree, &(), &LIMITS);
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::new(100.0, 4.0 * 20.0 + 40.0));
    }

    // Runs one frame, a layout pass plus a redraw update, against tree
    // with the given viewport.
    fn frame(
        element: &mut Element<'static, Never, Theme, ()>,
        tree: &mut Tree,
        viewport: &Rectangle,
    ) {
        element.layout(tree, &(), &LIMITS);
        let redraw = Event::Window(window::Event::RedrawRequested(
            std::time::Instant::now(),
        ));
        let mut bus: shell::Bus<Never> = shell::Bus::new();
        let mut shell = Shell::new(&window::Headless, shell::Waker::noop(), &mut bus);
        element.update(
            tree,
            &redraw,
            Layout::new(tree.size),
            mouse::Cursor::Unavailable,
            &(),
            &mut shell,
            viewport,
        );
    }

    // Like frame, but with the viewport anchored at the bottom of the
    // content, like the chat timeline's scrollable.
    fn frame_bottom(
        element: &mut Element<'static, Never, Theme, ()>,
        tree: &mut Tree,
        viewport_height: f32,
    ) {
        element.layout(tree, &(), &LIMITS);
        let total = tree.size.height;
        let viewport = Rectangle {
            x: 0.0,
            y: (total - viewport_height).max(0.0),
            width: 400.0,
            height: viewport_height,
        };
        frame(element, tree, &viewport);
    }

    // Asserts every visible row sits at its own offset and consecutive
    // rows are contiguous, with spacing added per-row at draw time.
    fn assert_rows_in_order(state: &State, spacing: f32) {
        let mut rows: Vec<(usize, Rectangle)> = state
            .visible_layouts
            .iter()
            .map(|(i, layout, _)| (*i, layout.bounds()))
            .collect();
        rows.sort_by_key(|(i, _)| *i);
        assert!(!rows.is_empty());
        for w in rows.windows(2) {
            assert_ne!(
                w[0].0, w[1].0,
                "row {} materialized twice",
                w[0].0
            );
            assert!(
                w[1].0 > w[0].0,
                "visible rows not sorted: {} then {}",
                w[0].0,
                w[1].0
            );
        }
        for (i, rect) in &rows {
            assert_eq!(rect.y, state.offsets[*i], "row {i} y");
        }
        for w in rows.windows(2) {
            assert!(
                (w[1].1.y - (w[0].1.y + w[0].1.height)).abs() < 0.01,
                "rows {} and {} not contiguous (spacing {spacing})",
                w[0].0,
                w[1].0
            );
        }
    }

    // assert_rows_in_order for lists of uniform-height rows.
    fn assert_uniform_rows_in_order(state: &State, spacing: f32, row_height: f32) {
        for (i, layout, _) in &state.visible_layouts {
            assert_eq!(layout.bounds().height, row_height, "row {i} height");
        }
        assert_rows_in_order(state, spacing);
    }

    #[test]
    fn appending_while_scrolled_to_bottom_keeps_every_row_in_order() {
        // Regression when sending a message the previously-last row was
        // drawn at a stale oversized offset, leaving a gap above it.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 100.0;

        let content = content_of(ROWS);
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, fixed_item).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        // Settle the initial compute, then run frames anchored at the
        // bottom, rebuilding the view every frame like the app does.
        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));
        let total_before = ROWS as f32 * 20.0 + (ROWS - 1) as f32 * SPACING;
        assert_eq!(tree.size, Size::new(100.0, total_before));
        assert!(!state_of(&tree).visible_layouts.is_empty());

        // Send a message, appending a row and queuing an update on the
        // new last row like the timeline does.
        shared.push(ROWS, ROWS);
        let _ = shared.get_index_mut(ROWS as usize);

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }

        let state = state_of(&tree);
        assert!(matches!(state.task, Task::Idle));
        let total_after = (ROWS + 1) as f32 * 20.0 + ROWS as f32 * SPACING;
        assert_eq!(tree.size, Size::new(100.0, total_after));

        // Offsets must be contiguous since they are pure cumulative row
        // heights, with spacing added per-row at draw time.
        assert_eq!(state.offsets.len(), ROWS as usize + 2);
        assert_eq!(state.offsets.last(), Some(&(total_after - ROWS as f32 * SPACING)));
        for i in 0..=ROWS as usize {
            let expected = i as f32 * 20.0;
            assert!(
                (state.offsets[i] - expected).abs() < 0.01,
                "offset[{i}] = {}, expected {expected}",
                state.offsets[i]
            );
        }

        // Every visible row must sit at its own offset and height, and
        // rows must stay contiguous, including the previously-last one.
        let mut rows: Vec<(usize, Rectangle)> = state
            .visible_layouts
            .iter()
            .map(|(i, layout, _)| (*i, layout.bounds()))
            .collect();
        rows.sort_by_key(|(i, _)| *i);
        assert_eq!(rows.last().unwrap().0, ROWS as usize, "new row visible");
        assert_uniform_rows_in_order(state, SPACING, 20.0);
    }

    #[test]
    fn replacing_the_new_last_row_keeps_the_previous_row_in_order() {
        // Sending a message appends a local echo that the sync later
        // replaces in place; the row before must keep height and position.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 100.0;

        let height_item =
            |_index: usize, _key: u8, item: Rc<u8>| -> Element<'static, Never, Theme, ()> {
                iced_widget::Space::new()
                    .width(100)
                    .height(*item as f32 * 20.0)
                    .boxed()
            };

        let content = Content::with_items((0..ROWS).map(|i| (i, 1u8)).collect());
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, height_item).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), height_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));

        // Local echo appended, then replaced by the confirmed event
        // with a different height, e.g. the send-state icon resolved.
        shared.push(ROWS, 1u8);
        let _ = shared.get_index_mut(ROWS as usize);
        for _ in 0..2 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), height_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }
        *shared.get_index_mut(ROWS as usize).unwrap() = 2;
        for _ in 0..2 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), height_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }

        let state = state_of(&tree);
        assert!(matches!(state.task, Task::Idle));
        assert_eq!(
            tree.size,
            Size::new(100.0, (ROWS as f32 * 20.0 + 40.0) + ROWS as f32 * SPACING)
        );
        // The previously-last row keeps its original height and offset.
        let prev = state
            .visible_layouts
            .iter()
            .find(|(i, _, _)| *i == ROWS as usize - 1)
            .map(|(_, l, _)| l.bounds());
        let prev = prev.expect("previous row still visible");
        assert_eq!(prev.height, 20.0, "previous row height");
        assert_eq!(prev.y, state.offsets[ROWS as usize - 1], "previous row y");
        assert_rows_in_order(state, SPACING);
    }

    #[test]
    fn appending_several_rows_in_one_frame_keeps_every_row_in_order() {
        // Several messages arriving in one sync diff are appended as a
        // batch that offsets, total size and visible rows all count.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 100.0;

        let content = content_of(ROWS);
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, fixed_item).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));

        // A batch of new messages, like one VectorDiff append.
        for i in ROWS..ROWS + 3 {
            shared.push(i, i);
        }

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }

        let state = state_of(&tree);
        let total = (ROWS + 3) as f32 * 20.0 + (ROWS + 2) as f32 * SPACING;
        assert_eq!(tree.size, Size::new(100.0, total));
        assert_eq!(state.offsets.len(), ROWS as usize + 4);
        for i in 0..=ROWS as usize + 3 {
            assert!(
                (state.offsets[i] - i as f32 * 20.0).abs() < 0.01,
                "offset[{i}] = {}",
                state.offsets[i]
            );
        }
        let mut rows: Vec<usize> = state.visible_layouts.iter().map(|(i, _, _)| *i).collect();
        rows.sort_unstable();
        assert_eq!(rows.last(), Some(&(ROWS as usize + 2)), "newest row visible");
        assert_uniform_rows_in_order(state, SPACING, 20.0);
    }

    #[test]
    fn inserting_at_front_while_scrolled_to_top_keeps_every_row_in_order() {
        // Paginating backwards prepends older messages; the rows visible
        // must be reindexed and repositioned, never lost or duplicated.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 100.0;

        let content = content_of(ROWS);
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, fixed_item).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        let viewport = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: VIEWPORT,
        };
        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame(&mut element, &mut tree, &viewport);
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));
        assert_eq!(tree.size, Size::new(100.0, ROWS as f32 * 20.0 + (ROWS - 1) as f32 * SPACING));

        // Older messages arrive at the front, keyed to sort before the
        // existing ones in the index map.
        for i in 0..3u8 {
            shared.insert(0, 100 + i, 0);
        }

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame(&mut element, &mut tree, &viewport);
        }

        let state = state_of(&tree);
        let total = (ROWS + 3) as f32 * 20.0 + (ROWS + 2) as f32 * SPACING;
        assert_eq!(tree.size, Size::new(100.0, total));
        assert_eq!(state.offsets.len(), ROWS as usize + 4);
        for i in 0..=ROWS as usize + 3 {
            assert!(
                (state.offsets[i] - i as f32 * 20.0).abs() < 0.01,
                "offset[{i}] = {}",
                state.offsets[i]
            );
        }
        // The viewport stayed at the top, so it now shows the three newly
        // inserted rows first; the old rows shifted to indices 3+.
        let first = state.visible_layouts.first().map(|(i, _, _)| *i);
        assert_eq!(first, Some(0), "viewport at top shows the new front rows");
        assert_uniform_rows_in_order(state, SPACING, 20.0);
    }

    #[test]
    fn removing_a_visible_row_keeps_the_rest_in_order() {
        // Removing a row from the middle, e.g. a redaction, must shrink
        // offsets and size by that exact row and keep the rest in order.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 400.0; // everything visible

        let content = content_of(ROWS);
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, fixed_item).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        let viewport = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: VIEWPORT,
        };
        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame(&mut element, &mut tree, &viewport);
        }
        assert_eq!(state_of(&tree).visible_layouts.len(), ROWS as usize);

        let removed = shared.remove(4).unwrap();
        assert_eq!(*removed, 4);

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame(&mut element, &mut tree, &viewport);
        }

        let state = state_of(&tree);
        assert_eq!(tree.size, Size::new(100.0, 9.0 * 20.0 + 8.0 * SPACING));
        assert_eq!(state.offsets.len(), 10);
        for i in 0..=9 {
            assert!(
                (state.offsets[i] - i as f32 * 20.0).abs() < 0.01,
                "offset[{i}] = {}",
                state.offsets[i]
            );
        }
        // Nine rows remain, reindexed contiguously so the former index 5
        // row now sits at index 4.
        assert_eq!(state.visible_layouts.len(), 9);
        let mut indices: Vec<usize> = state.visible_layouts.iter().map(|(i, _, _)| *i).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..9).collect::<Vec<_>>());
        assert_uniform_rows_in_order(state, SPACING, 20.0);
    }

    #[test]
    fn updating_a_visible_middle_row_repositions_the_following_rows() {
        // A visible middle row growing, e.g. an image finishing loading,
        // must push every later row down by exactly the difference.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 400.0; // everything visible

        let height_item =
            |_index: usize, _key: u8, item: Rc<u8>| -> Element<'static, Never, Theme, ()> {
                iced_widget::Space::new()
                    .width(100)
                    .height(*item as f32 * 20.0)
                    .boxed()
            };

        let content = Content::with_items((0..ROWS).map(|i| (i, 1u8)).collect());
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, height_item).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        let viewport = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: VIEWPORT,
        };
        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), height_item).spacing(SPACING).into();
            frame(&mut element, &mut tree, &viewport);
        }
        assert_eq!(tree.size, Size::new(100.0, ROWS as f32 * 20.0 + (ROWS - 1) as f32 * SPACING));

        // Row 3 triples in height.
        *shared.get_index_mut(3).unwrap() = 3;

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), height_item).spacing(SPACING).into();
            frame(&mut element, &mut tree, &viewport);
        }

        let state = state_of(&tree);
        assert_eq!(
            tree.size,
            Size::new(100.0, (ROWS as f32 * 20.0 + 40.0) + (ROWS - 1) as f32 * SPACING)
        );
        // Rows before 3 are untouched; rows after 3 shift down by 40.
        assert!((state.offsets[3] - 60.0).abs() < 0.01);
        assert!((state.offsets[4] - 120.0).abs() < 0.01, "offset[4] = {}", state.offsets[4]);
        assert!(
            (state.offsets[10] - (10.0 * 20.0 + 40.0)).abs() < 0.01,
            "total offset = {}",
            state.offsets[10]
        );
        assert_rows_in_order(state, SPACING);
        // Row 3 itself must carry its new height in the visible layouts.
        let row3 = state
            .visible_layouts
            .iter()
            .find(|(i, _, _)| *i == 3)
            .map(|(_, l, _)| l.bounds().height);
        assert_eq!(row3, Some(60.0));
    }

    #[test]
    fn popping_the_last_row_shrinks_total_and_keeps_rows_in_order() {
        // Truncating from the end, e.g. a VectorDiff truncate, must drop
        // exactly the removed rows from the offsets and the total size.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 100.0;

        let content = content_of(ROWS);
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, fixed_item).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }
        assert!(matches!(state_of(&tree).task, Task::Idle));

        shared.remove(9);
        shared.remove(8);

        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }

        let state = state_of(&tree);
        assert_eq!(tree.size, Size::new(100.0, 8.0 * 20.0 + 7.0 * SPACING));
        assert_eq!(state.offsets.len(), 9);
        assert!(
            (state.offsets[8] - 8.0 * 20.0).abs() < 0.01,
            "total offset = {}",
            state.offsets[8]
        );
        assert_uniform_rows_in_order(state, SPACING, 20.0);
    }

    #[test]
    fn appending_inside_anchored_scrollable_keeps_rows_in_order() {
        // Closest to the real chat timeline, a list inside a scrollable
        // with a bounded viewport where appending must stay exact.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 30;
        let window_limits = layout::Limits::new(
            Size::ZERO,
            Size {
                width: 400.0,
                height: 100.0,
            },
        );
        let window_viewport = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 100.0,
        };

        let content = content_of(ROWS);
        let mut shared = content.clone();
        let mut tree = {
            let list: Element<'static, Never, Theme, ()> =
                List::new(content, fixed_item).spacing(SPACING).into();
            let mut scroller: Element<'static, Never, Theme, ()> =
                iced_widget::Scrollable::new(list)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .anchor_bottom()
                    .boxed();
            let mut tree = Tree::new(&scroller);
            scroller.diff(&mut tree);
            tree
        };

        let mut run_frame = |shared: &Content<u8, u8>, tree: &mut Tree| {
            let list: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), fixed_item).spacing(SPACING).into();
            let mut scroller: Element<'static, Never, Theme, ()> =
                iced_widget::Scrollable::new(list)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .anchor_bottom()
                    .boxed();
            scroller.layout(tree, &(), &window_limits);
            let redraw = Event::Window(window::Event::RedrawRequested(
                std::time::Instant::now(),
            ));
            let mut bus: shell::Bus<Never> = shell::Bus::new();
            let mut shell = Shell::new(&window::Headless, shell::Waker::noop(), &mut bus);
            scroller.update(
                tree,
                &redraw,
                Layout::new(tree.size),
                mouse::Cursor::Unavailable,
                &(),
                &mut shell,
                &window_viewport,
            );
        };

        for _ in 0..5 {
            run_frame(&shared, &mut tree);
        }
        assert!(matches!(
            tree.children[0].state.downcast_ref::<State>().task,
            Task::Idle
        ));

        // Send a message.
        shared.push(ROWS, ROWS);
        let _ = shared.get_index_mut(ROWS as usize);
        for _ in 0..5 {
            run_frame(&shared, &mut tree);
        }

        let state = tree.children[0].state.downcast_ref::<State>();
        assert!(matches!(state.task, Task::Idle));
        let total = (ROWS + 1) as f32 * 20.0 + ROWS as f32 * SPACING;
        assert_eq!(state.offsets.len(), ROWS as usize + 2);
        assert!(
            (state.offsets[ROWS as usize + 1] - total + ROWS as f32 * SPACING).abs() < 0.01,
            "total offset = {}",
            state.offsets[ROWS as usize + 1]
        );
        for i in 0..=ROWS as usize + 1 {
            assert!(
                (state.offsets[i] - i as f32 * 20.0).abs() < 0.01,
                "offset[{i}] = {}",
                state.offsets[i]
            );
        }
        let mut rows: Vec<usize> = state.visible_layouts.iter().map(|(i, _, _)| *i).collect();
        rows.sort_unstable();
        assert_eq!(rows.last(), Some(&(ROWS as usize)), "new row materialized");
        assert_uniform_rows_in_order(state, SPACING, 20.0);
    }

    #[test]
    fn appended_rows_remeasure_stale_last_row_heights() {
        // Regression where the newest row keeps its bottom padding baked
        // into its stored height after a newer row is appended.
        const SPACING: f32 = 8.0;
        const ROWS: u8 = 10;
        const VIEWPORT: f32 = 100.0;
        const LAST_ROW_PADDING: f32 = 36.0;

        // Row height depends on the last row, the only one that carries
        // the timeline's bottom padding.
        let len = Rc::new(std::cell::Cell::new(ROWS as usize));
        let make_view = |len: Rc<std::cell::Cell<usize>>| {
            move |index: usize, _key: u8, _item: Rc<u8>| -> Element<'static, Never, Theme, ()> {
                iced_widget::Space::new()
                    .width(100)
                    .height(if index == len.get() - 1 {
                        20.0 + LAST_ROW_PADDING
                    } else {
                        20.0
                    })
                    .boxed()
            }
        };

        let content = content_of(ROWS);
        let mut shared = content.clone();
        let mut tree = {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(content, make_view(len.clone())).spacing(SPACING).into();
            let mut tree = Tree::new(&element);
            element.diff(&mut tree);
            tree
        };

        // Settle with all ten rows materialized at the bottom.
        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), make_view(len.clone())).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }
        let state = state_of(&tree);
        let last_height = state
            .visible_layouts
            .iter()
            .find(|(i, _, _)| *i == ROWS as usize - 1)
            .map(|(_, l, _)| l.bounds().height);
        assert_eq!(last_height, Some(20.0 + LAST_ROW_PADDING), "last row measured with padding");

        // Send a message so the previous row is no longer last.
        shared.push(ROWS, ROWS);
        len.set(ROWS as usize + 1);
        for _ in 0..3 {
            let mut element: Element<'static, Never, Theme, ()> =
                List::new(shared.clone(), make_view(len.clone())).spacing(SPACING).into();
            frame_bottom(&mut element, &mut tree, VIEWPORT);
        }

        let state = state_of(&tree);
        // Only the newest row may carry the bottom padding; the previously
        // last row must have been re-measured without it.
        let new_total =
            ROWS as f32 * 20.0 + (20.0 + LAST_ROW_PADDING) + ROWS as f32 * SPACING;
        assert_eq!(
            tree.size,
            Size::new(100.0, new_total),
            "phantom padding must not linger in the total"
        );
        let prev = state
            .visible_layouts
            .iter()
            .find(|(i, _, _)| *i == ROWS as usize - 1);
        let prev = prev
            .map(|(_, l, _)| l.bounds())
            .expect("previously-last row stays visible");
        assert_eq!(prev.height, 20.0, "previously-last row re-measured without padding");
        assert_eq!(prev.y, state.offsets[ROWS as usize - 1], "previously-last row y");
    }
}
