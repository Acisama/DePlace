use std::cell::{Cell, RefCell};
use std::time::{Duration, SystemTime};

use deplace_core::formatting::{format_elapsed_time, format_elapsed_time_hms};
use iced::advanced::text::{self, Paragraph, Renderer as _, paragraph};
use iced::advanced::widget::{
    Meta, Tree,
    text::{self as text_widget, Catalog as _},
    tree,
};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, renderer};
use iced::{
    Color, Element, Event, Font, Length, Pixels, Rectangle, Renderer, Size, Widget as _, window,
};

/// Displays the whole number of seconds elapsed since `since`, e.g. "12s".
///
/// Restyles the same way `w::Text` does (`.color()`, `.size()`, `.font()`),
/// but unlike a plain `Text` it reschedules its own redraw for the next
/// second boundary every time it draws, so the count keeps ticking up on
/// its own instead of only updating when something else causes a redraw.
///
/// `draw` re-shapes its cached paragraph itself (through the `RefCell`/
/// `Cell` below) rather than only relying on `layout` having already done
/// so: virtualized lists like `sweeten`'s only call `layout` on an item
/// again once *they* consider it changed, so a `shell.invalidate_layout`
/// from deep inside a static list row isn't guaranteed to be acted on. This
/// way the text itself never freezes even if the claimed layout space goes
/// briefly stale; `update` still calls `invalidate_layout` when the text's
/// width class changes (digit count going up/down) so that containers which
/// *do* honor it give the widget the right amount of room.
pub struct ElapsedTime {
    since: SystemTime,
    mode: Mode,
    size: Option<Pixels>,
    color: Option<Color>,
    font: Option<Font>,
}

/// How the elapsed duration is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// e.g. "1h 23m 45s", each unit shown only if non-zero.
    #[default]
    Compact,
    /// e.g. "1:23:45", with the hours and/or minutes component omitted
    /// entirely when zero.
    Hms,
}

pub fn elapsed_time(since: SystemTime) -> ElapsedTime {
    ElapsedTime {
        since,
        mode: Mode::default(),
        size: None,
        color: None,
        font: None,
    }
}

impl ElapsedTime {
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

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

    fn format_secs(&self, secs: u64) -> String {
        match self.mode {
            Mode::Compact => format_elapsed_time(secs),
            Mode::Hms => format_elapsed_time_hms(secs),
        }
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
    paragraph: RefCell<paragraph::Plain<P>>,
    // Sentinel so the very first draw always (re-)shapes the text, even if
    // it happens to land on an elapsed count of 0.
    last_secs: Cell<u64>,
}

impl<P: Paragraph> Default for State<P> {
    fn default() -> Self {
        Self {
            paragraph: RefCell::new(paragraph::Plain::default()),
            last_secs: Cell::new(u64::MAX),
        }
    }
}

impl Meta for ElapsedTime {}

impl<Message> Widget<Message, iced::Theme, Renderer> for ElapsedTime {
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
        let content = self.format_secs(secs);

        let state = tree
            .state
            .downcast_ref::<State<<Renderer as text::Renderer>::Paragraph>>();
        state.last_secs.set(secs);

        let mut paragraph = state.paragraph.borrow_mut();

        tree.size = text_widget::layout(&mut paragraph, renderer, limits, &content, self.format());
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

        let secs = self.elapsed().as_secs();

        if state.last_secs.get() != secs {
            state.last_secs.set(secs);

            let format = self.format();
            let bounds = layout.bounds().size();
            let content = self.format_secs(secs);

            let _ = state.paragraph.borrow_mut().update(text::Text {
                content: &content,
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
        }

        text_widget::draw(
            renderer,
            style,
            layout.bounds(),
            state.paragraph.borrow().raw(),
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

        // Most ticks just swap digits in place, which `draw` handles itself
        // by reshaping its cached paragraph without a relayout. But the text
        // also *grows or shrinks* at specific boundaries (9s -> 10s,
        // 59s -> 1m0s, 9m -> 10m, 59m -> 1h0m, 9h -> 10h, ...), and those
        // need a real relayout so the widget claims the right amount of
        // space. Comparing formatted lengths catches exactly those
        // boundaries without hardcoding each one.
        let state = tree
            .state
            .downcast_ref::<State<<Renderer as text::Renderer>::Paragraph>>();
        if self.format_secs(state.last_secs.get()).len() != self.format_secs(secs).len() {
            shell.invalidate_layout();
        }

        let until_next_second = Duration::from_secs(1)
            .checked_sub(Duration::from_nanos(u64::from(elapsed.subsec_nanos())))
            .unwrap_or_default();
        shell.request_redraw_at(*now + until_next_second);
    }
}

impl<'a, Message: 'a> From<ElapsedTime> for Element<'a, Message> {
    fn from(value: ElapsedTime) -> Self {
        value.boxed()
    }
}
