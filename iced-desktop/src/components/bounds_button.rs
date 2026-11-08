use iced::{
    Background, Color, Element, Event, Length, Padding, Rectangle, Renderer, Size, Theme, Vector,
    Widget as _,
    advanced::{
        Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Meta, Operation, Tree, tree},
    },
    touch,
    widget::button::{Catalog, Status, Style, StyleFn},
    window,
};

use crate::common::DePlaceWidget;

pub struct BoundsButton<Message, Content>
where
    Content: DePlaceWidget<Message>,
{
    content: Content,

    on_press: Option<Box<dyn Fn(Rectangle) -> Message + 'static>>,
    on_enter: Option<Box<dyn Fn(Rectangle) -> Message + 'static>>,
    on_exit: Option<Box<dyn Fn(Rectangle) -> Message + 'static>>,

    width: Length,
    height: Length,
    padding: Padding,
    class: StyleFn<'static, Theme>,
}

struct State {
    is_hovered: bool,
    is_pressed: bool,
    status: Status,
}

pub fn bounds_button<Message, Content: DePlaceWidget<Message>>(
    content: Content,
) -> BoundsButton<Message, Content> {
    BoundsButton::new(content)
}

impl<Message, Content: DePlaceWidget<Message>> BoundsButton<Message, Content> {
    pub fn new(content: Content) -> Self {
        Self {
            content,
            on_enter: None,
            on_exit: None,
            on_press: None,
            width: Length::Shrink,
            height: Length::Shrink,
            padding: 0.0.into(),
            class: Box::new(|_theme, _status| Style::default()),
        }
    }

    pub fn on_enter(mut self, f: impl Fn(Rectangle) -> Message + 'static) -> Self {
        self.on_enter = Some(Box::new(f));
        self
    }

    pub fn on_exit(mut self, f: impl Fn(Rectangle) -> Message + 'static) -> Self {
        self.on_exit = Some(Box::new(f));
        self
    }

    pub fn on_press(mut self, f: impl Fn(Rectangle) -> Message + 'static) -> Self {
        self.on_press = Some(Box::new(f));
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

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'static) -> Self {
        self.class = Box::new(style);
        self
    }
}

impl<Message, Content: DePlaceWidget<Message>> Meta for BoundsButton<Message, Content> {}

impl<Message, Content: DePlaceWidget<Message>> Widget<Message, Theme, Renderer>
    for BoundsButton<Message, Content>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            is_hovered: false,
            is_pressed: false,
            status: Status::Active,
        })
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
        if shell.is_event_captured() {
            return;
        }

        let state: &mut State = tree.state.downcast_mut();
        let bounds = layout.bounds();
        let is_hovered = cursor.is_over(bounds);

        if is_hovered != state.is_hovered {
            state.is_hovered = is_hovered;

            if is_hovered && let Some(on_enter) = &self.on_enter {
                shell.publish(on_enter(bounds));
            } else if let Some(on_exit) = &self.on_exit {
                shell.publish(on_exit(bounds));
            }
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if self.on_press.is_some() && cursor.is_over(bounds) =>
            {
                state.is_pressed = true;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                if state.is_pressed {
                    state.is_pressed = false;

                    if cursor.is_over(bounds)
                        && let Some(on_press) = &self.on_press
                    {
                        shell.publish(on_press(bounds));
                    }

                    shell.capture_event();
                }
            }
            Event::Touch(touch::Event::FingerLost { .. }) => {
                state.is_pressed = false;
            }
            _ => {}
        }

        let target = if self.on_press.is_none() {
            Status::Disabled
        } else if cursor.is_over(bounds) {
            if state.is_pressed {
                Status::Pressed
            } else {
                Status::Hovered
            }
        } else {
            Status::Active
        };

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            state.status = target;
        } else if target != state.status {
            shell.request_redraw();
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
        _style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();

        let style = theme.style(&self.class, state.status);

        if style.background.is_some() || style.border.width > 0.0 || style.shadow.color.a > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: style.border,
                    shadow: style.shadow,
                    snap: style.snap,
                },
                style
                    .background
                    .unwrap_or(Background::Color(Color::TRANSPARENT)),
            );
        }

        self.content.draw(
            &tree.children[0],
            renderer,
            theme,
            &renderer::Style {
                text_color: style.text_color,
            },
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

impl<'a, Message, Content> From<BoundsButton<Message, Content>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Content: DePlaceWidget<Message> + 'a,
{
    fn from(widget: BoundsButton<Message, Content>) -> Self {
        widget.boxed()
    }
}
