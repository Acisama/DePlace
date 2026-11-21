use std::marker::PhantomData;
use std::time::{Duration, Instant};

use iced::advanced::widget::{Meta, Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, renderer};
use iced::widget as w;
use iced::widget::text::LineHeight;
use iced::{
    Color, Element, Event, Font, Length, Padding, Pixels, Rectangle, Renderer, Size, Theme,
    Widget as _, touch, window,
};

use crate::helpers::{blend_color, smoothstep};

use super::link::link;
use super::{CopyTextExt, phosphor_icon};

/// A text button that, on click, publishes the message
/// [`CopyTextExt::copy_text`] builds from its `copy_text` — the caller's
/// `Message` decides what that actually does (clipboard write, toast,
/// whatever) — then flashes `lit_color` and eases back to `normal_color`
/// as feedback, independent of whatever the click actually triggered.
///
/// The button itself is just a normal `w::container(w::text(..))`, rebuilt
/// each time it's drawn with whatever the current eased background color
/// is; this widget only owns the click handling and the flash timer.
pub struct CopyButton<Message> {
    text: String,
    copy_text: String,
    normal_color: Color,
    lit_color: Color,
    text_size: f32,
    size: Option<Pixels>,
    font: Option<Font>,
    padding: Padding,
    duration: Duration,
    _message: PhantomData<Message>,
}

pub fn copy_button<Message>(
    text: impl Into<String>,
    copy_text: impl Into<String>,
    normal_color: impl Into<Color>,
    lit_color: impl Into<Color>,
    text_size: f32,
) -> CopyButton<Message> {
    CopyButton {
        text: format!("{} ", text.into()),
        copy_text: copy_text.into(),
        normal_color: normal_color.into(),
        lit_color: lit_color.into(),
        text_size,
        size: None,
        font: None,
        padding: Padding::new(6.0),
        duration: Duration::from_millis(400),
        _message: PhantomData,
    }
}

impl<Message> CopyButton<Message> {
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }

    pub fn font(mut self, font: impl Into<Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// How long the flash takes to ease back to `normal_color`.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    fn background(&self, flash: Option<Instant>) -> Color {
        match flash {
            Some(started) => {
                let t =
                    (started.elapsed().as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);
                blend_color(self.lit_color, self.normal_color, smoothstep(t))
            }
            None => self.normal_color,
        }
    }
}

impl<'a, Message: CopyTextExt + 'a> CopyButton<Message> {
    /// Builds the (plain `w::container(w::text(..))`) visuals for the given
    /// background, re-created on every `layout`/`draw`/`operate` call so the
    /// background can track the live flash state without this widget having
    /// to hand-roll any text shaping or quad drawing of its own.
    fn build_content(
        &self,
        text_color: Color,
        copied: bool,
        text_size: f32,
    ) -> Element<'a, Message> {
        let mut text = w::text(self.text.clone())
            .color(text_color)
            .line_height(LineHeight::Relative(1.0))
            .size(text_size);

        if let Some(size) = self.size {
            text = text.size(size);
        }

        text = text.font_maybe(self.font);

        link(
            w::row![
                text,
                phosphor_icon(
                    if copied {
                        phosphor_svgs::icon::check::BOLD
                    } else {
                        phosphor_svgs::icon::clipboard_text::BOLD
                    },
                    text_size
                )
                .color(text_color)
            ],
            text_color,
        )
        .boxed()
    }
}

#[derive(Default)]
struct State {
    is_pressed: bool,
    has_copied: bool,
    // When the most recent click fired, so `draw` can ease the background
    // from `lit_color` back to `normal_color`. Never reset to `None`;
    // `draw` just stops animating once `duration` has elapsed.
    flash: Option<Instant>,
}

impl<Message> Meta for CopyButton<Message> {}

impl<Message> Widget<Message, Theme, Renderer> for CopyButton<Message>
where
    Message: CopyTextExt,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_ref::<State>();
        let mut content = self.build_content(self.normal_color, state.has_copied, self.text_size);
        tree.diff_children(std::slice::from_mut(&mut content));
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let state = tree.state.downcast_ref::<State>();
        let mut content = self.build_content(self.normal_color, state.has_copied, self.text_size);
        content.layout(&mut tree.children[0], renderer, limits);
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
        let state = tree.state.downcast_mut::<State>();

        let mut content = self.build_content(self.normal_color, state.has_copied, self.text_size);
        content.update(
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

        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if cursor.is_over(bounds) =>
            {
                state.is_pressed = true;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                if state.is_pressed {
                    state.is_pressed = false;

                    if cursor.is_over(bounds) {
                        shell.publish(Message::copy_text(self.copy_text.clone()));
                        state.flash = Some(Instant::now());
                        state.has_copied = true;
                        shell.request_redraw();
                    }

                    shell.capture_event();
                }
            }
            Event::Touch(touch::Event::FingerLost { .. }) => {
                state.is_pressed = false;
            }
            Event::Window(window::Event::RedrawRequested(_))
                if state
                    .flash
                    .is_some_and(|started| started.elapsed() < self.duration) =>
            {
                shell.request_redraw();
            }
            _ => {}
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
        let content = self.build_content(
            self.background(state.flash),
            state.has_copied,
            self.text_size,
        );

        content.draw(
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
        let state = tree.state.downcast_ref::<State>();
        let mut content = self.build_content(self.normal_color, state.has_copied, self.text_size);
        content.operate(&mut tree.children[0], layout, viewport, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message> From<CopyButton<Message>> for Element<'a, Message>
where
    Message: CopyTextExt + 'static,
{
    fn from(value: CopyButton<Message>) -> Self {
        value.boxed()
    }
}
