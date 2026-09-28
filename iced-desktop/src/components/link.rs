use iced::{
    Color, Element, Event, Length, Rectangle, Size,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
};

/// Wraps `content`, drawing an underline under it and switching to a pointer
/// cursor whenever it's hovered, and calling `on_press` with the content's
/// own (freshly laid-out) bounds when it's clicked.
///
/// Unlike `track_bounds`, this needs no state kept outside the widget itself:
/// hover is derived from the cursor position on every `draw`/`mouse_interaction`
/// call, and the bounds handed to `on_press` are read straight from `layout`
/// at the moment of the click.
pub struct Link<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    underline_color: Color,
    on_press: Box<dyn Fn(Rectangle) -> Message + 'a>,
}

pub fn link<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    underline_color: Color,
    on_press: impl Fn(Rectangle) -> Message + 'a,
) -> Link<'a, Message, Theme, Renderer> {
    Link {
        content: content.into(),
        underline_color,
        on_press: Box::new(on_press),
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Link<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);

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
        self.content.as_widget_mut().update(
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
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
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
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            viewport,
            renderer,
            operation,
        );
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
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
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
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Link<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(widget: Link<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
