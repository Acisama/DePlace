use std::time::{Duration, Instant};

use iced::advanced::text::{self, Paragraph, Renderer as _, paragraph};
use iced::advanced::widget::{
    Meta, Tree,
    text::{self as text_widget, Catalog as _},
    tree,
};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, renderer};
use iced::{
    Color, Element, Event, Font, Length, Pixels, Rectangle, Renderer, Size, Vector, Widget as _,
    window,
};

use crate::helpers::{blend_color, smoothstep};

/// Displays a number, and when it changes, slides the old value out and the
/// new value in — the same "odometer tick" Discord uses for member/reaction
/// counts: counting up slides upward, counting down slides downward.
///
/// Restyles like `w::Text` (`.color()`, `.size()`, `.font()`). The slide
/// distance is the text's own line height, so it stays in bounds as long as
/// nothing clips the widget tighter than one line.
pub struct AnimatedNumber {
    value: i64,
    size: Option<Pixels>,
    color: Option<Color>,
    font: Option<Font>,
    duration: Duration,
}

pub fn animated_number(value: i64) -> AnimatedNumber {
    AnimatedNumber {
        value,
        size: None,
        color: None,
        font: None,
        duration: Duration::from_millis(180),
    }
}

impl AnimatedNumber {
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }

    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn font(mut self, font: impl Into<Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    /// How long the slide/fade between two values takes.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    fn format(&self) -> text_widget::Format {
        text_widget::Format {
            size: self.size,
            font: self.font,
            ..text_widget::Format::default()
        }
    }
}

struct State<P: Paragraph> {
    // The value currently on screen.
    current: paragraph::Plain<P>,
    // The value being slid away from, only meaningful while `transition` is `Some`.
    previous: paragraph::Plain<P>,
    // The last value `current` was shaped for, so a change can be detected
    // without re-shaping every frame.
    committed: Option<i64>,
    // When the running transition started, and its direction: `1.0` while
    // counting up (old slides down and out, new slides down into place),
    // `-1.0` while counting down (both slide the other way).
    transition: Option<(Instant, f32)>,
}

impl<P: Paragraph> Default for State<P> {
    fn default() -> Self {
        Self {
            current: paragraph::Plain::default(),
            previous: paragraph::Plain::default(),
            committed: None,
            transition: None,
        }
    }
}

impl Meta for AnimatedNumber {}

impl<Message> Widget<Message, iced::Theme, Renderer> for AnimatedNumber {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fit,
            height: Length::Fit,
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<<Renderer as text::Renderer>::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<<Renderer as text::Renderer>::Paragraph>::default())
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let content = self.value.to_string();

        let state = tree
            .state
            .downcast_mut::<State<<Renderer as text::Renderer>::Paragraph>>();

        tree.size = text_widget::layout(
            &mut state.current,
            renderer,
            limits,
            &content,
            self.format(),
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        _cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };

        let state = tree
            .state
            .downcast_mut::<State<<Renderer as text::Renderer>::Paragraph>>();

        if state.committed != Some(self.value) {
            if let Some(previous_value) = state.committed {
                let bounds = layout.bounds().size();
                let format = self.format();
                let previous_text = previous_value.to_string();
                let current_text = self.value.to_string();

                let _ = state.previous.update(text::Text {
                    content: &previous_text,
                    bounds,
                    size: format.size.unwrap_or_else(|| renderer.text_size()),
                    line_height: format.line_height.unwrap_or_else(|| renderer.line_height()),
                    font: format.font.unwrap_or_else(|| renderer.font()),
                    align_x: format.align_x,
                    align_y: format.align_y,
                    shaping: format.shaping,
                    wrapping: format.wrapping,
                    ellipsis: format.ellipsis,
                    hint_factor: renderer.hint_factor(),
                });

                let direction = if self.value > previous_value {
                    -1.0
                } else {
                    1.0
                };
                state.transition = Some((*now, direction));

                if previous_text.len() != current_text.len() {
                    shell.invalidate_layout();
                }
            }

            state.committed = Some(self.value);
        }

        if state
            .transition
            .is_some_and(|(started, _)| now.duration_since(started) < self.duration)
        {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree
            .state
            .downcast_ref::<State<<Renderer as text::Renderer>::Paragraph>>();

        let bounds = layout.bounds();
        let base_color = self.color.unwrap_or(style.text_color);

        let Some((started, direction)) = state.transition else {
            draw_paragraph(
                renderer,
                style,
                bounds,
                state.current.raw(),
                base_color,
                theme,
                viewport,
            );
            return;
        };

        let t = (started.elapsed().as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);

        if t >= 1.0 {
            draw_paragraph(
                renderer,
                style,
                bounds,
                state.current.raw(),
                base_color,
                theme,
                viewport,
            );
            return;
        }

        let t = smoothstep(t);
        let line_height = state
            .current
            .min_bounds()
            .height
            .max(state.previous.min_bounds().height);

        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };

        renderer.with_layer(clip, |renderer| {
            renderer.with_translation(Vector::new(0.0, -direction * t * line_height), |renderer| {
                let color = blend_color(base_color, Color::TRANSPARENT, t);
                draw_paragraph(
                    renderer,
                    style,
                    bounds,
                    state.previous.raw(),
                    color,
                    theme,
                    viewport,
                );
            });

            renderer.with_translation(
                Vector::new(0.0, direction * (1.0 - t) * line_height),
                |renderer| {
                    let color = blend_color(Color::TRANSPARENT, base_color, t);
                    draw_paragraph(
                        renderer,
                        style,
                        bounds,
                        state.current.raw(),
                        color,
                        theme,
                        viewport,
                    );
                },
            );
        });
    }
}

fn draw_paragraph<P: Paragraph>(
    renderer: &mut Renderer,
    style: &renderer::Style,
    bounds: Rectangle,
    paragraph: &P,
    color: Color,
    theme: &iced::Theme,
    viewport: &Rectangle,
) where
    Renderer: text::Renderer<Paragraph = P>,
{
    text_widget::draw(
        renderer,
        style,
        bounds,
        paragraph,
        text_widget::Style {
            color: Some(color),
            ..text_widget::Style::default()
        },
        theme.selection(),
        viewport,
    );
}

impl<'a, Message: 'a> From<AnimatedNumber> for Element<'a, Message> {
    fn from(value: AnimatedNumber) -> Self {
        value.boxed()
    }
}
