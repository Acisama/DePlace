use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Instant;

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, path::Arc};
use iced::{Color, Element, Event, Radians, Rectangle, Renderer, Theme, mouse};

pub struct PieProgress {
    pub value: f32, // 0.0..=1.0
    pub color: Color,
    pub thickness: f32,
}

#[derive(Debug, Clone, Default)]
pub struct PieState {
    hovered: bool,
    hover_changed: Option<Instant>,
}

impl<Message> canvas::Program<Message> for PieProgress {
    type State = PieState;

    fn update(
        &self,
        state: &mut PieState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Event::Window(iced::window::Event::RedrawRequested(_)) = event
            && state
                .hover_changed
                .is_some_and(|at| at.elapsed().as_secs_f32() < 0.1)
        {
            return Some(canvas::Action::request_redraw());
        }

        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
            let hovered = cursor.is_over(bounds);
            if hovered != state.hovered {
                state.hovered = hovered;
                state.hover_changed = Some(Instant::now());
                return Some(canvas::Action::request_redraw());
            }
        }
        None
    }

    fn draw(
        &self,
        state: &PieState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let edge = bounds.width.min(bounds.height) / 2.0 - 0.5;

        // Animation progress: 0.0 = not hovered, 1.0 = fully hovered
        let t = state
            .hover_changed
            .map(|at| (at.elapsed().as_secs_f32() / 0.1).min(1.0))
            .unwrap_or(1.0);
        let t = t * t * (3.0 - 2.0 * t); // smoothstep easing
        let p = if state.hovered { t } else { 1.0 - t };

        let thickness = self.thickness * (1.0 + p); // thickness -> 2 * thickness
        let ring_radius = edge - thickness / 2.0; // grow inward, never clipped
        let inner = edge - 2.0 * thickness - thickness / 2.0; // gap grows with the ring
        let color = self.color;

        // Outline ring
        frame.stroke(
            &Path::circle(center, ring_radius),
            Stroke::default().with_width(thickness).with_color(color),
        );

        // Filled pie, sweeping from empty to a full circle as value goes 0 -> 1
        let v = self.value.clamp(0.0, 1.0);
        if v >= 1.0 {
            frame.fill(&Path::circle(center, inner), color);
        } else if v > 0.0 {
            let start = -FRAC_PI_2; // 12 o'clock
            let pie = Path::new(|b| {
                // `arc` starts its own sub-path at the arc's start point, so the
                // center has to be connected explicitly before closing back to it.
                b.arc(Arc {
                    center,
                    radius: inner,
                    start_angle: Radians(start),
                    end_angle: Radians(start + v * TAU),
                });
                b.line_to(center);
                b.close();
            });
            frame.fill(&pie, color);
        }

        vec![frame.into_geometry()]
    }
}

pub fn pie_progress<'a, Message: 'a>(
    value: f32,
    size: f32,
    color: Color,
    thickness: f32,
) -> Element<'a, Message> {
    Canvas::new(PieProgress {
        value,
        color,
        thickness,
    })
    .width(size)
    .height(size)
    .into()
}
