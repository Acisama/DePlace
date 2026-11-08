use iced::time::Instant;
use iced::widget::canvas::{self, Canvas, Frame, Text};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Theme, Widget as _, mouse};

use crate::MONO_FONT;

#[derive(Debug, Clone, Copy, Default)]
pub struct FpsState {
    last_frame: Option<Instant>,
    fps: f32,
}

pub struct FpsCounter {
    color: Color,
}

impl<Message> canvas::Program<Message> for FpsCounter {
    type State = FpsState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Event::Window(iced::window::Event::RedrawRequested(now)) = event {
            if let Some(last) = state.last_frame {
                let delta = now.duration_since(last).as_secs_f32();
                if delta > 0.0 {
                    let current_fps = 1.0 / delta;
                    // Exponential Moving Average for a readable, smooth value
                    state.fps = (state.fps * 0.9) + (current_fps * 0.1);
                }
            }
            state.last_frame = Some(*now);

            // Keeps the widget rendering continuously at the monitor's refresh rate
            return Some(canvas::Action::request_redraw());
        }

        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        frame.fill_text(Text {
            content: format!("FPS: {:.0}", state.fps.round()),
            color: self.color,
            position: Point::new(0.0, 0.0),
            size: bounds.height.into(),
            font: MONO_FONT,
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }
}

/// Helper that creates the FPS counter widget
pub fn fps_counter<'a, Message: 'a>(
    color: impl Into<Color>,
    width: impl Into<Length>,
    height: impl Into<Length>,
) -> Element<'a, Message> {
    Canvas::new(FpsCounter {
        color: color.into(),
    })
    .width(width)
    .height(height)
    .boxed()
}
