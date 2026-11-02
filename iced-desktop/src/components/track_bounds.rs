use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, Widget as _,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Meta, Operation, Tree, tree},
    },
    window,
};

use crate::common::DePlaceWidget;

pub struct TrackBounds<Message, Content: DePlaceWidget<Message>> {
    content: Content,
    on_change: Box<dyn Fn(Rectangle) -> Message + 'static>,
}

pub fn track_bounds<Message, Content: DePlaceWidget<Message>>(
    content: Content,
    on_change: impl Fn(Rectangle) -> Message + 'static,
) -> TrackBounds<Message, Content> {
    TrackBounds {
        content,
        on_change: Box::new(on_change),
    }
}

#[derive(Default)]
struct State {
    last_bounds: Option<Rectangle>,
}

impl<Message, Content: DePlaceWidget<Message>> Meta for TrackBounds<Message, Content> {}

impl<Message, Content: DePlaceWidget<Message>> Widget<Message, Theme, Renderer>
    for TrackBounds<Message, Content>
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
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let state: &mut State = tree.state.downcast_mut();
            let bounds = layout.bounds(); // absolute window coordinates, same as Sensor uses

            if state.last_bounds != Some(bounds) {
                state.last_bounds = Some(bounds);
                shell.publish((self.on_change)(bounds));
            }
        }

        self.content.update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
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

impl<'a, Message, Content> From<TrackBounds<Message, Content>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Content: DePlaceWidget<Message> + 'a,
{
    fn from(widget: TrackBounds<Message, Content>) -> Self {
        widget.boxed()
    }
}
