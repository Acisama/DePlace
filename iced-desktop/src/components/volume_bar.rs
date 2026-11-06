use std::sync::Arc;

use deplace_core::call::VolumeCell;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path};
use iced::{Color, Element, Event, Point, Rectangle, Renderer, Size, Theme, Widget as _, mouse};

/// A level meter for a live volume the audio thread keeps updating.
///
/// Takes the volume as `f32` bits in an [`AtomicU32`], the same
/// representation the call thread publishes, the bar loads the latest value on every frame. Each
///
/// The fill is a horizontal bar: full and green at maximum volume,
/// emptier and redder the quieter it gets.
pub struct VolumeBar {
    volume: Arc<VolumeCell>,
    track: Color,
}

impl<Message> canvas::Program<Message> for VolumeBar {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            return Some(canvas::Action::request_redraw());
        }

        None
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = frame.size();

        let radius = size.height.min(size.width) / 2.0;

        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, radius.into()),
            self.track,
        );

        let volume = self.volume.get().clamp(0.0, 1.0);
        let fill_width = (size.width * volume).max(radius * 2.0).min(size.width);
        let fill = Path::rounded_rectangle(
            Point::ORIGIN,
            Size::new(fill_width, size.height),
            radius.into(),
        );

        frame.fill(&fill, Color::from_rgb(1.0 - volume, volume, 0.0));

        vec![frame.into_geometry()]
    }
}

/// Helper that creates a volume bar
pub fn volume_bar<'a, Message: 'a>(
    volume: Arc<VolumeCell>,
    width: f32,
    height: f32,
    track: Color,
) -> Element<'a, Message> {
    Canvas::new(VolumeBar { volume, track })
        .width(width)
        .height(height)
        .boxed()
}
