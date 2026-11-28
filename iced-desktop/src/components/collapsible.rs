use std::time::{Duration, Instant};

use iced::advanced::widget::{Meta, Operation, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, Widget as _, window};

use crate::common::DePlaceWidget;
use crate::helpers::{lerp_size, smoothstep};

/// Which dimension(s) [`Collapsible`] eases towards nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
    Both,
}

/// Wraps `content` so that it eases between fully shown and fully hidden as
/// `active` flips.
///
/// The *layout* size always stays at `content`'s full, uncollapsed size,
/// only the drawn portion shrinks towards its center down
/// to nothing.
pub struct Collapsible<Message, Content: DePlaceWidget<Message>> {
    content: Content,
    active: bool,
    axis: Axis,
    duration: Duration,
    _marker: std::marker::PhantomData<Message>,
}

pub fn collapsible<Message, Content: DePlaceWidget<Message>>(
    content: Content,
    active: bool,
) -> Collapsible<Message, Content> {
    Collapsible {
        content,
        active,
        axis: Axis::Both,
        duration: Duration::from_millis(150),
        _marker: std::marker::PhantomData,
    }
}

impl<Message, Content: DePlaceWidget<Message>> Collapsible<Message, Content> {
    /// Which dimension(s) to ease towards nothing. Defaults to [`Axis::Both`].
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// How long the grow/shrink transition takes.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

#[derive(Default)]
struct State {
    // Whether we've ever laid out - the first frame snaps to `active`
    // instead of animating in from zero.
    initialized: bool,
    // The `active` value the current/last transition headed towards.
    active: bool,
    // Current eased progress: `0.0` fully collapsed, `1.0` full size.
    progress: f32,
    // The progress we're easing away from, and when that leg started.
    transition: Option<(f32, Instant)>,
}

/// The portion of `bounds` currently shown, centered within it and shrunk
/// towards nothing (along `axis`) as `progress` drops to `0.0`.
fn visible_bounds(bounds: Rectangle, progress: f32, axis: Axis) -> Rectangle {
    let full = Size::new(bounds.width, bounds.height);
    let collapsed = match axis {
        Axis::Horizontal => Size::new(0.0, full.height),
        Axis::Vertical => Size::new(full.width, 0.0),
        Axis::Both => Size::ZERO,
    };

    let size = lerp_size(collapsed, full, progress);

    Rectangle {
        x: bounds.x + (bounds.width - size.width) / 2.0,
        y: bounds.y + (bounds.height - size.height) / 2.0,
        width: size.width,
        height: size.height,
    }
}

impl<Message, Content: DePlaceWidget<Message>> Meta for Collapsible<Message, Content> {}

impl<Message, Content: DePlaceWidget<Message>> Widget<Message, Theme, Renderer>
    for Collapsible<Message, Content>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));

        let state = tree.state.downcast_mut::<State>();

        if !state.initialized {
            state.initialized = true;
            state.active = self.active;
            state.progress = if self.active { 1.0 } else { 0.0 };
        }
    }

    fn size(&self) -> Size<Length> {
        self.content.size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let child = &mut tree.children[0];
        self.content.layout(child, renderer, limits);
        tree.size = child.size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() else {
            return;
        };

        self.content
            .operate(tree, layout, viewport, renderer, operation);
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
        let bounds = layout.bounds();
        let Some((content_layout, content_tree)) = layout.iter_mut(&mut tree.children).next()
        else {
            return;
        };

        // Only the portion of the content currently revealed should react
        // to the cursor - a partially collapsed widget shouldn't respond to
        // clicks/hovers over its hidden part.
        let progress = tree.state.downcast_ref::<State>().progress;
        let visible = visible_bounds(bounds, progress, self.axis);

        let masked_cursor = match cursor.position_over(visible) {
            Some(position) => mouse::Cursor::Available(position),
            None => cursor.obstruct(),
        };

        self.content.update(
            content_tree,
            event,
            content_layout,
            masked_cursor,
            renderer,
            shell,
            viewport,
        );

        if shell.is_event_captured() {
            return;
        }

        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };

        let state = tree.state.downcast_mut::<State>();

        if self.active != state.active {
            state.transition = Some((state.progress, *now));
            state.active = self.active;
        }

        if let Some((from, start)) = state.transition {
            let t = (now.duration_since(start).as_secs_f32() / self.duration.as_secs_f32())
                .clamp(0.0, 1.0);
            let to = if state.active { 1.0 } else { 0.0 };
            state.progress = from + (to - from) * smoothstep(t);

            if t >= 1.0 {
                state.transition = None;
            } else {
                shell.request_redraw();
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
        let progress = tree.state.downcast_ref::<State>().progress;
        let visible = visible_bounds(layout.bounds(), progress, self.axis);

        let Some(clip) = visible.intersection(viewport) else {
            return;
        };
        let Some((content_layout, content_tree)) = layout.iter(&tree.children).next() else {
            return;
        };

        let masked_cursor = match cursor.position_over(visible) {
            Some(position) => mouse::Cursor::Available(position),
            None => cursor.obstruct(),
        };

        renderer.with_layer(clip, |renderer| {
            self.content.draw(
                content_tree,
                renderer,
                theme,
                style,
                content_layout,
                masked_cursor,
                &clip,
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let Some((content_layout, content_tree)) = layout.iter(&tree.children).next() else {
            return mouse::Interaction::default();
        };

        let progress = tree.state.downcast_ref::<State>().progress;
        let visible = visible_bounds(layout.bounds(), progress, self.axis);

        let masked_cursor = match cursor.position_over(visible) {
            Some(position) => mouse::Cursor::Available(position),
            None => cursor.obstruct(),
        };

        self.content.mouse_interaction(
            content_tree,
            content_layout,
            masked_cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        let bounds = layout.bounds();
        let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() else {
            return Vec::new();
        };
        let viewport = viewport.intersection(&bounds).unwrap_or(*viewport);

        self.content
            .overlay(tree, layout, renderer, &viewport, translation, window)
    }
}

impl<'a, Message, Content> From<Collapsible<Message, Content>>
    for Element<'a, Message, iced::Theme, iced::Renderer>
where
    Message: 'a,
    Content: DePlaceWidget<Message> + 'a,
{
    fn from(widget: Collapsible<Message, Content>) -> Self {
        widget.boxed()
    }
}
