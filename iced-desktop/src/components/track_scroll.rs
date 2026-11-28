use iced::{
    Event, Length, Rectangle, Renderer, Size, Theme, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{
            Meta, Operation, Tree,
            operation::{self, Outcome, Scrollable as ScrollableOperationState},
            tree,
        },
    },
    widget::Id,
    window,
};

use crate::common::DePlaceWidget;

/// Scroll state of a scrollable, as reported to [`track_scroll`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMetrics {
    /// How far the content is scrolled past the top, in pixels.
    pub translation: f32,
    /// Height of the content, which the scroll range follows.
    pub content: f32,
}

struct FindScrolledFromTop {
    target: Id,
    value: Option<ScrollMetrics>,
}

impl Operation<ScrollMetrics> for FindScrolledFromTop {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<ScrollMetrics>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _bounds: Rectangle,
        content: Size,
        translation: Vector,
        _state: &mut dyn ScrollableOperationState,
    ) {
        if id == Some(&self.target) {
            self.value = Some(ScrollMetrics {
                translation: translation.y,
                content: content.height,
            });
        }
    }

    fn finish(&self) -> Outcome<ScrollMetrics> {
        match self.value {
            Some(v) => Outcome::Some(v),
            None => Outcome::None,
        }
    }
}

pub struct TrackScroll<Message, Content: DePlaceWidget<Message>> {
    content: Content,
    scrollable_id: iced::widget::Id,
    on_change: Box<dyn Fn(ScrollMetrics) -> Message + 'static>,
}

pub fn track_scroll<Message, Content: DePlaceWidget<Message>>(
    content: Content,
    scrollable_id: iced::widget::Id,
    on_change: impl Fn(ScrollMetrics) -> Message + 'static,
) -> TrackScroll<Message, Content> {
    TrackScroll {
        content,
        scrollable_id,
        on_change: Box::new(on_change),
    }
}

#[derive(Default)]
struct State {
    last_value: Option<(f32, f32)>,
    last_tick: Option<std::time::Instant>,
}

impl<Message, Content: DePlaceWidget<Message>> Meta for TrackScroll<Message, Content> {}

impl<Message, Content: DePlaceWidget<Message>> Widget<Message, Theme, Renderer>
    for TrackScroll<Message, Content>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content.layout(&mut tree.children[0], renderer, limits);

        tree.size = tree.children[0].size;
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
        self.content.update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        if let Event::Window(window::Event::RedrawRequested(tick)) = event {
            let state: &mut State = tree.state.downcast_mut();
            if state.last_tick == Some(*tick) {
                return;
            }
            state.last_tick = Some(*tick);

            let mut operation = FindScrolledFromTop {
                target: self.scrollable_id.clone(),
                value: None,
            };
            self.content.operate(
                &mut tree.children[0],
                layout,
                viewport,
                renderer,
                &mut operation::black_box(&mut operation),
            );

            if let Some(value) = operation.value {
                // Rounded to whole pixels, both jitter by sub-pixel amounts
                // every frame while animating
                let rounded = (value.translation.round(), value.content.round());
                let state: &mut State = tree.state.downcast_mut();
                if state.last_value != Some(rounded) {
                    state.last_value = Some(rounded);
                    shell.publish((self.on_change)(value));
                }
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
        self.content.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .operate(&mut tree.children[0], layout, viewport, renderer, operation);
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
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
        self.content.overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}
