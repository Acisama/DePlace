use iced::{
    Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector, Widget as _,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
};

use crate::common::DePlaceWidget;

/// Wraps `content` and emits `on_right_press` with the position of the
/// right click in window coordinates.
///
/// Sweeten's mouse area reports right click positions relative to its
/// own bounds, which is useless for menus anchored anywhere on the
/// window. This wrapper is that mouse area with its positions
/// reabsoluted by the area's own layout origin.
pub struct RightClickArea<Message, Content: DePlaceWidget<Message>> {
    content: Content,
    on_right_press: Box<dyn Fn(Point) -> Message + 'static>,
}

/// Wraps `content` and calls `on_right_press` with the position of the
/// right click in window coordinates.
pub fn right_click<Message, Content: DePlaceWidget<Message>>(
    content: Content,
    on_right_press: impl Fn(Point) -> Message + 'static,
) -> RightClickArea<Message, Content> {
    RightClickArea {
        content,
        on_right_press: Box::new(on_right_press),
    }
}

impl<Message, Content: DePlaceWidget<Message>> iced::advanced::widget::Meta
    for RightClickArea<Message, Content>
{
}

impl<Message, Content: DePlaceWidget<Message>> Widget<Message, Theme, Renderer>
    for RightClickArea<Message, Content>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && let Some(position) = cursor.position_in(layout.bounds())
        {
            // Window coordinates, so menus anchored anywhere open
            // under the cursor instead of under the area's origin
            let origin = layout.bounds().position();
            shell.publish((self.on_right_press)(Point::new(
                origin.x + position.x,
                origin.y + position.y,
            )));
            shell.capture_event();
            return;
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

impl<'a, Message, Content> From<RightClickArea<Message, Content>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Content: DePlaceWidget<Message> + 'a,
{
    fn from(widget: RightClickArea<Message, Content>) -> Self {
        widget.boxed()
    }
}
