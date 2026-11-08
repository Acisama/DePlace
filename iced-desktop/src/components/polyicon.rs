use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::time::Instant;

use deplace_core::polyicon::PolyIcon;
use iced::advanced::graphics::geometry::Frame;
use iced::advanced::mouse;
use iced::widget::Canvas;
use iced::widget::canvas::{self, Geometry, Path, Stroke};
use iced::{Color, Element, Event, Point, Rectangle, Renderer, Theme, Vector, Widget};

pub struct IcedPolyIcon {
    content: PolyIcon,
    color: Color,
    background: Color,
}

#[derive(Debug, Clone, Default)]
pub struct PolyIconState {
    hovered: bool,
    hover_changed: Option<Instant>,
}

/// Builds a regular polygon with the given number of `sides`, starting the
/// first vertex at `start_angle`. Shapes with fewer than 3 sides (our `0`
/// placeholder for "no polygon") fall back to a circle.
fn regular_polygon_path(center: Point, radius: f32, sides: u8, start_angle: f32) -> Path {
    if sides < 3 {
        return Path::circle(center, radius);
    }

    Path::new(|builder| {
        for i in 0..sides {
            let angle = start_angle + TAU * i as f32 / sides as f32;
            let point = center + Vector::new(angle.cos(), angle.sin()) * radius;

            if i == 0 {
                builder.move_to(point);
            } else {
                builder.line_to(point);
            }
        }
        builder.close();
    })
}

/// Lays out `count` dots (0, 1, or 2) symmetrically around the center.
fn dot_offsets(count: u8, spread: f32) -> Vec<Vector> {
    match count {
        0 => Vec::new(),
        1 => vec![Vector::new(0.0, 0.0)],
        _ => vec![Vector::new(-spread, 0.0), Vector::new(spread, 0.0)],
    }
}

/// How close a regular polygon's edge comes to its center, as a fraction of
/// its circumradius (the radius its vertices sit on). `1.0` for a circle.
fn apothem_factor(sides: u8) -> f32 {
    if sides < 3 {
        1.0
    } else {
        (PI / sides as f32).cos()
    }
}

impl<Message> canvas::Program<Message> for IcedPolyIcon {
    type State = PolyIconState;

    fn update(
        &self,
        state: &mut PolyIconState,
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
        state: &PolyIconState,
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

        const UP: f32 = -FRAC_PI_2; // point a vertex to 12 o'clock

        // The stroke straddles its path by half its width, so pull the path
        // in by that much or the outer half of the border gets clipped by
        // the canvas bounds.
        let stroke_width = edge / 8.0;
        let outer_radius = edge - stroke_width / 2.0;

        let outer_rotation = UP - p * FRAC_PI_2;
        let outline_path =
            regular_polygon_path(center, outer_radius, self.content.outline_shape, outer_rotation);
        frame.fill(&outline_path, self.background);
        frame.stroke(
            &outline_path,
            Stroke::default()
                .with_color(self.color)
                .with_width(stroke_width),
        );

        // Closest the stroked outline ever comes to the center (at its
        // flattest point, not its vertices), minus a fixed gap. Sizing the
        // inner shape's circumradius to this guarantees it never touches the
        // outline, however few sides either shape has.
        let outline_clearance =
            outer_radius * apothem_factor(self.content.outline_shape) - stroke_width / 2.0;
        let gap = outer_radius * 0.08;
        let max_inner_radius = (outline_clearance - gap).max(0.0);

        let inner_radius = max_inner_radius * (0.9 + 0.1 * p);
        let inner_rotation = UP + PI / self.content.inner_shape.max(1) as f32 + p * FRAC_PI_2;
        frame.fill(
            &regular_polygon_path(
                center,
                inner_radius,
                self.content.inner_shape,
                inner_rotation,
            ),
            self.color,
        );

        // Same trick for the dots against the inner shape's own edges.
        let dot_area_radius = inner_radius * apothem_factor(self.content.inner_shape);
        let dot_radius = dot_area_radius * 0.22;
        for offset in dot_offsets(self.content.dot_count, dot_area_radius * 0.45) {
            frame.fill(&Path::circle(center + offset, dot_radius), self.background);
        }

        vec![frame.into_geometry()]
    }
}

pub fn polyicon<'a, M: 'a>(
    content: PolyIcon,
    size: f32,
    color: impl Into<Color>,
    background: impl Into<Color>,
) -> Element<'a, M> {
    Canvas::new(IcedPolyIcon {
        content,
        color: color.into(),
        background: background.into(),
    })
    .width(size)
    .height(size)
    .boxed()
}
