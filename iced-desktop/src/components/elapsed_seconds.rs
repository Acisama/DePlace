use std::time::{Duration, SystemTime};

use iced::advanced::text::{self, Paragraph, paragraph};
use iced::advanced::widget::{
    Meta, Tree,
    text::{self as text_widget, Catalog as _},
    tree,
};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, renderer};
use iced::{
    Color, Element, Event, Font, Length, Pixels, Rectangle, Renderer, Size, Widget as _, window,
};

/// Displays the whole number of seconds elapsed since `since`, e.g. "12s".
///
/// Restyles the same way `w::Text` does (`.color()`, `.size()`, `.font()`),
/// but unlike a plain `Text` it reschedules its own redraw for the next
/// second boundary every time it draws, so the count keeps ticking up on
/// its own instead of only updating when something else causes a redraw.
pub struct ElapsedSeconds {
    since: SystemTime,
    size: Option<Pixels>,
    color: Option<Color>,
    font: Option<Font>,
}

pub fn elapsed_seconds(since: SystemTime) -> ElapsedSeconds {
    ElapsedSeconds {
        since,
        size: None,
        color: None,
        font: None,
    }
}

impl ElapsedSeconds {
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

    fn elapsed(&self) -> Duration {
        self.since.elapsed().unwrap_or_default()
    }
}

struct State<P: Paragraph> {
    paragraph: paragraph::Plain<P>,
    // Sentinel so the very first redraw always re-shapes the text, even
    // if it happens to land on an elapsed count of 0.
    last_secs: u64,
}

impl<P: Paragraph> Default for State<P> {
    fn default() -> Self {
        Self {
            paragraph: paragraph::Plain::default(),
            last_secs: u64::MAX,
        }
    }
}

impl Meta for ElapsedSeconds {}

impl<Message> Widget<Message, iced::Theme, Renderer> for ElapsedSeconds {
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
        let secs = self.elapsed().as_secs();
        let content = format!("{secs}s");

        let state = tree
            .state
            .downcast_mut::<State<<Renderer as text::Renderer>::Paragraph>>();
        state.last_secs = secs;

        tree.size = text_widget::layout(
            &mut state.paragraph,
            renderer,
            limits,
            &content,
            text_widget::Format {
                size: self.size,
                font: self.font,
                ..text_widget::Format::default()
            },
        );
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

        text_widget::draw(
            renderer,
            style,
            layout.bounds(),
            state.paragraph.raw(),
            text_widget::Style {
                color: self.color,
                ..text_widget::Style::default()
            },
            theme.selection(),
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };

        let elapsed = self.elapsed();
        let secs = elapsed.as_secs();

        let state = tree
            .state
            .downcast_mut::<State<<Renderer as text::Renderer>::Paragraph>>();

        if state.last_secs != secs {
            state.last_secs = secs;
            shell.invalidate_layout();
        }

        let until_next_second = Duration::from_secs(1)
            .checked_sub(Duration::from_nanos(u64::from(elapsed.subsec_nanos())))
            .unwrap_or_default();
        shell.request_redraw_at(*now + until_next_second);
    }
}

impl<'a, Message: 'a> From<ElapsedSeconds> for Element<'a, Message> {
    fn from(value: ElapsedSeconds) -> Self {
        value.boxed()
    }
}
