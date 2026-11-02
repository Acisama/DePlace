use iced::{
    Color, Element, Event, Length, Rectangle, Renderer, Size, Widget as _,
    advanced::{
        Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Meta, Operation, Tree},
    },
};

use crate::common::DePlaceWidget;

/// Wraps `content`, drawing an underline under it and switching to a pointer
/// cursor whenever it's hovered, and calling `on_press` with the content's
/// own (freshly laid-out) bounds when it's clicked.
///
/// Unlike `track_bounds`, this needs no state kept outside the widget itself:
/// hover is derived from the cursor position on every `draw`/`mouse_interaction`
/// call, and the bounds handed to `on_press` are read straight from `layout`
/// at the moment of the click.
pub struct Link<Message, Content: DePlaceWidget<Message>> {
    content: Content,
    underline_color: Color,
    on_press: Box<dyn Fn(Rectangle) -> Message + 'static>,
}

pub fn link<Message, Content: DePlaceWidget<Message>>(
    content: Content,
    underline_color: Color,
    on_press: impl Fn(Rectangle) -> Message + 'static,
) -> Link<Message, Content> {
    Link {
        content,
        underline_color,
        on_press: Box::new(on_press),
    }
}

impl<Message, Content: DePlaceWidget<Message>> Meta for Link<Message, Content> {}

impl<Message, Content: DePlaceWidget<Message>> Widget<Message, iced::Theme, Renderer>
    for Link<Message, Content>
{
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

        if shell.is_event_captured() {
            return;
        }

        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.is_over(layout.bounds())
        {
            shell.publish((self.on_press)(layout.bounds()));
            shell.capture_event();
        }
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
        self.content.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );

        if cursor.is_over(layout.bounds()) {
            let bounds = layout.bounds();

            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: bounds.y + bounds.height - 1.0,
                        width: bounds.width,
                        height: 1.0,
                    },
                    ..Default::default()
                },
                self.underline_color,
            );
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
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            self.content
                .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: iced::Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, Renderer>> {
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

impl<'a, Message, Content: DePlaceWidget<Message>> From<Link<Message, Content>>
    for Element<'a, Message, iced::Theme, Renderer>
where
    Message: 'a,
    Content: DePlaceWidget<Message> + 'a,
{
    fn from(widget: Link<Message, Content>) -> Self {
        widget.boxed()
    }
}
