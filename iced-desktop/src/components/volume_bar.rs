use std::sync::Arc;

use deplace_core::call::VolumeCell;
use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Event, Point, Rectangle, Renderer, Size, Theme, Widget as _, mouse};

/// Which way the level last moved, so callers can e.g. animate attack/decay
/// differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VolumeDirection {
    #[default]
    Steady,
    Rising,
    Falling,
}

/// Which edge of the bar the fill grows from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Right,
    Left,
    Bottom,
    Top,
}

/// Per-frame state for a [`VolumeBar`]: the last sample read from the
/// [`VolumeCell`] and the direction it moved in since the previous frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct VolumeBarState {
    volume: f32,
    direction: VolumeDirection,
}

impl VolumeBarState {
    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn direction(&self) -> VolumeDirection {
        self.direction
    }
}

/// A level meter for a live volume the audio thread keeps updating.
///
/// Takes the volume as `f32` bits in an [`AtomicU32`], the same
/// representation the call thread publishes; the bar loads the latest value
/// on every frame.
///
/// The fill is a horizontal bar, interpolating from `start_color` at zero
/// volume to `end_color` at full volume, over a `bg_color` track.
pub struct VolumeBar {
    volume: Arc<VolumeCell>,
    direction: Direction,
    start_color: Color,
    end_color: Color,
    bg_color: Color,
    border_radius: f32,
    border_color: Color,
    border_width: f32,
}

impl<Message> canvas::Program<Message> for VolumeBar {
    type State = VolumeBarState;

    fn update(
        &self,
        state: &mut VolumeBarState,
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            let volume = self.volume.get().clamp(0.0, 1.0);

            state.direction = if volume > state.volume {
                VolumeDirection::Rising
            } else if volume < state.volume {
                VolumeDirection::Falling
            } else {
                VolumeDirection::Steady
            };
            state.volume = volume;

            return Some(canvas::Action::request_redraw());
        }

        None
    }

    fn draw(
        &self,
        state: &VolumeBarState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = frame.size();

        // Inset by half the stroke width so the border is drawn fully
        // inside the canvas bounds instead of being clipped on the outer
        // half, and shrink the radius to match so the border still looks
        // concentric with the background instead of chunkier at the
        // corners.
        let inset = self.border_width / 2.0;
        let bg_origin = Point::new(inset, inset);
        let bg_size = Size::new(
            (size.width - self.border_width).max(0.0),
            (size.height - self.border_width).max(0.0),
        );
        let bg_radius = (self.border_radius - inset).max(0.0);

        let outline = Path::rounded_rectangle(bg_origin, bg_size, bg_radius.into());

        frame.fill(&outline, self.bg_color);

        let volume = state.volume();
        if volume > 0.0 {
            let (fill_origin, fill_size) = match self.direction {
                Direction::Right => {
                    let length = (bg_size.width * volume)
                        .max(bg_radius * 2.0)
                        .min(bg_size.width);
                    (bg_origin, Size::new(length, bg_size.height))
                }
                Direction::Left => {
                    let length = (bg_size.width * volume)
                        .max(bg_radius * 2.0)
                        .min(bg_size.width);
                    (
                        Point::new(bg_origin.x + bg_size.width - length, bg_origin.y),
                        Size::new(length, bg_size.height),
                    )
                }
                Direction::Bottom => {
                    let length = (bg_size.height * volume)
                        .max(bg_radius * 2.0)
                        .min(bg_size.height);
                    (bg_origin, Size::new(bg_size.width, length))
                }
                Direction::Top => {
                    let length = (bg_size.height * volume)
                        .max(bg_radius * 2.0)
                        .min(bg_size.height);
                    (
                        Point::new(bg_origin.x, bg_origin.y + bg_size.height - length),
                        Size::new(bg_size.width, length),
                    )
                }
            };
            let fill = Path::rounded_rectangle(fill_origin, fill_size, bg_radius.into());

            let center = Point::new(
                fill_origin.x + fill_size.width / 2.0,
                fill_origin.y + fill_size.height / 2.0,
            );
            let (base, tip) = match self.direction {
                Direction::Right => (
                    Point::new(fill_origin.x, center.y),
                    Point::new(fill_origin.x + fill_size.width, center.y),
                ),
                Direction::Left => (
                    Point::new(fill_origin.x + fill_size.width, center.y),
                    Point::new(fill_origin.x, center.y),
                ),
                Direction::Bottom => (
                    Point::new(center.x, fill_origin.y),
                    Point::new(center.x, fill_origin.y + fill_size.height),
                ),
                Direction::Top => (
                    Point::new(center.x, fill_origin.y + fill_size.height),
                    Point::new(center.x, fill_origin.y),
                ),
            };

            let end_color = lerp_color(self.start_color, self.end_color, volume);

            frame.fill(
                &fill,
                Linear::new(base, tip)
                    .add_stop(0.0, self.start_color)
                    .add_stop(1.0, end_color),
            );
        }

        if self.border_width > 0.0 {
            frame.stroke(
                &outline,
                Stroke::default()
                    .with_color(self.border_color)
                    .with_width(self.border_width),
            );
        }

        vec![frame.into_geometry()]
    }
}

fn lerp_color(start: Color, end: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: start.r + (end.r - start.r) * t,
        g: start.g + (end.g - start.g) * t,
        b: start.b + (end.b - start.b) * t,
        a: start.a + (end.a - start.a) * t,
    }
}

/// Helper that creates a volume bar
#[allow(clippy::too_many_arguments)]
pub fn volume_bar<'a, Message: 'a>(
    volume: Arc<VolumeCell>,
    width: f32,
    height: f32,
    direction: Direction,
    start_color: Color,
    end_color: Color,
    bg_color: Color,
    border_radius: f32,
    border_color: Color,
    border_width: f32,
) -> Element<'a, Message> {
    Canvas::new(VolumeBar {
        volume,
        direction,
        start_color,
        end_color,
        bg_color,
        border_radius,
        border_color,
        border_width,
    })
    .width(width)
    .height(height)
    .boxed()
}
