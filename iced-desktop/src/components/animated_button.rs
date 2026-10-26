use std::time::{Duration, Instant};

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::button::{self, Catalog, Status, Style, StyleFn};
use iced::{
    Background, Color, Element, Event, Length, Padding, Rectangle, Size, Vector, touch, window,
};

/// A [`button`](iced::widget::button) that eases its background and text
/// color across [`Status`] changes instead of snapping to them.
///
/// Accepts the same `style` closures as a regular button (`Fn(&Theme,
/// Status) -> Style`); the widget just samples that function at both the
/// status it's transitioning *from* and the one it's transitioning *to* and
/// lerps between the two results every frame.
pub struct AnimatedButton<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Theme: Catalog,
{
    content: Element<'a, Message, Theme, Renderer>,
    on_press: Option<Message>,
    width: Length,
    height: Length,
    padding: Padding,
    duration: Duration,
    class: Theme::Class<'a>,
}

impl<'a, Message, Theme, Renderer> AnimatedButton<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Theme: Catalog,
{
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_press: None,
            width: Length::Fit,
            height: Length::Fit,
            padding: button::DEFAULT_PADDING,
            duration: Duration::from_millis(100),
            class: Theme::default(),
        }
    }

    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
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

    /// How long a transition between two [`Status`]es takes.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct State {
    is_pressed: bool,
    // The status we last committed to (and are drawing towards).
    status: Status,
    // The status we're easing away from, and when that transition started.
    transition: Option<(Status, Instant)>,
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for AnimatedButton<'a, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
    Theme: Catalog,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            is_pressed: false,
            status: Status::Active,
            transition: None,
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));

        let size = self.content.as_widget().size();
        self.width = self.width.stack(size.width);
        self.height = self.height.stack(size.height);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        layout::padded(
            tree,
            limits,
            self.width,
            self.height,
            self.padding,
            |tree, limits| {
                self.content.as_widget_mut().layout(tree, renderer, limits);

                tree.size
            },
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
        operation.container(None, layout.bounds(), viewport);
        operation.traverse(&mut |operation| {
            let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() else {
                return;
            };

            self.content
                .as_widget_mut()
                .operate(tree, layout, viewport, renderer, operation);
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
        let Some((content_layout, content_tree)) = layout.iter_mut(&mut tree.children).next()
        else {
            return;
        };

        self.content.as_widget_mut().update(
            content_tree,
            event,
            content_layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        if shell.is_event_captured() {
            return;
        }

        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();

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
                        && let Some(message) = self.on_press.clone()
                    {
                        shell.publish(message);
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
            if target != state.status {
                state.transition = Some((state.status, Instant::now()));
                state.status = target;
            }

            if state
                .transition
                .is_some_and(|(_, started)| started.elapsed() < self.duration)
            {
                shell.request_redraw();
            }
        } else if target != state.status {
            shell.request_redraw();
        }
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

        let target_style = theme.style(&self.class, state.status);
        let style = match state.transition {
            Some((from_status, started)) if started.elapsed() < self.duration => {
                let t = started.elapsed().as_secs_f32() / self.duration.as_secs_f32();
                let t = smoothstep(t.clamp(0.0, 1.0));
                let from_style = theme.style(&self.class, from_status);

                Style {
                    background: lerp_background(from_style.background, target_style.background, t),
                    text_color: lerp_color(from_style.text_color, target_style.text_color, t),
                    ..target_style
                }
            }
            _ => target_style,
        };

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

        let Some((content_layout, content_tree)) = layout.iter(&tree.children).next() else {
            return;
        };

        self.content.as_widget().draw(
            content_tree,
            renderer,
            theme,
            &renderer::Style {
                text_color: style.text_color,
            },
            content_layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) && self.on_press.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
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
        let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() else {
            return vec![];
        };

        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation, window)
    }
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

/// Lerps solid-color backgrounds channel-wise, treating `None` as fully
/// transparent so fading in/out a background looks like a fade rather than a
/// pop. Gradients aren't interpolable this way, so a transition into or out
/// of one just snaps halfway through instead.
fn lerp_background(from: Option<Background>, to: Option<Background>, t: f32) -> Option<Background> {
    match (from, to) {
        (Some(Background::Color(from)), Some(Background::Color(to))) => {
            Some(Background::Color(lerp_color(from, to, t)))
        }
        (None, Some(Background::Color(to))) => {
            Some(Background::Color(lerp_color(Color::TRANSPARENT, to, t)))
        }
        (Some(Background::Color(from)), None) => {
            Some(Background::Color(lerp_color(from, Color::TRANSPARENT, t)))
        }
        (from, to) => {
            if t < 0.5 {
                from
            } else {
                to
            }
        }
    }
}

impl<'a, Message, Theme, Renderer> From<AnimatedButton<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: Catalog + 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(button: AnimatedButton<'a, Message, Theme, Renderer>) -> Self {
        Element::new(button)
    }
}

pub fn animated_button<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> AnimatedButton<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Theme: Catalog,
{
    AnimatedButton::new(content)
}
