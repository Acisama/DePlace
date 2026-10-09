//! Help mode for iced 0.15-dev (master): dims the window, outlines the
//! hovered element with its own corner radius and shows a caption.
//!
//! Targets can be nested. The innermost target under the cursor wins, so a
//! chat target that contains message targets is shown when hovering its
//! padding or the gaps between messages, and a message target is shown
//! when hovering the message itself.

use std::borrow::Cow;
use std::cell::Cell;

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Shell, Widget};
use iced::border::Radius;
use iced::{
    Border, Color, Element, Event, Length, Pixels, Point, Rectangle, Size, Vector, alignment,
    keyboard, mouse, touch,
};

use super::home::HelpKey;

/// `K` identifies a target. It must be unique per element: all targets whose
/// key equals `hovered` are highlighted, so give each message its own key,
/// e.g. `HelpKey::Message(event_id)`.
#[derive(Debug, Clone, Copy, Hash)]
pub struct HelpState<K> {
    pub active: bool,
    pub hovered: Option<K>,
}

impl<K> Default for HelpState<K> {
    fn default() -> Self {
        Self {
            active: false,
            hovered: None,
        }
    }
}

/// Where the caption goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Caption {
    /// Below the element, or above it if there's no room below.
    #[default]
    Outside,
    /// Centered inside the element if it fits (wrapping as needed),
    /// otherwise falls back to `Outside`.
    Inside,
}

// Set by the innermost `HelpTarget` under the cursor during one CursorMoved
// pass. `shell.capture_event()` can't be used for this: when a base widget
// captures an event, iced drops the overlay for that frame (flicker).
thread_local! {
    static CLAIMED: Cell<bool> = const { Cell::new(false) };
}

// Highest ancestor `position` seen while building this frame's overlays
// (reset once per frame by `HelpRoot::overlay`). Since overlays are built
// bottom-up, a parent doesn't know the chain's total depth yet when it's
// constructed -- only once the outermost ancestor has run does this hold
// the true max, which is fine because drawing always happens after the
// whole tree's overlays have been built.
thread_local! {
    static MAX_POSITION: Cell<usize> = const { Cell::new(0) };
}

const DIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);
/// How much of the accent color the outermost (least saturated) ancestor in
/// a chain keeps; the rest is `offline_color`.
const BASE_OUTLINE_SATURATION: f32 = 0.35;
const BORDER_WIDTH: f32 = 2.0;

fn blend(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

// ---------------------------------------------------------------- HelpTarget

pub struct HelpTarget<'a, K, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    key: K,
    text: Cow<'a, str>,
    radius: RadiusSource<'a, Theme>,
    caption: Caption,
    accent: Color,
    text_color: Color,
    text_bg: Color,
    /// Blend target for a parent outline's desaturation -- see
    /// `BASE_OUTLINE_SATURATION`.
    offline_color: Color,
    active: bool,
    is_current: bool,
    on_hover: fn(Option<K>) -> Message,
}

enum RadiusSource<'a, Theme> {
    Fixed(Radius),
    Themed(Box<dyn Fn(&Theme) -> Radius + 'a>),
}

pub fn help<'a, K: PartialEq, Message, Theme, Renderer>(
    state: HelpState<K>,
    theme: deplace_core::theme::Theme,
    key: K,
    text: impl Into<Cow<'a, str>>,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    on_hover: fn(Option<K>) -> Message,
) -> HelpTarget<'a, K, Message, Theme, Renderer> {
    let is_current = state.active && state.hovered.as_ref() == Some(&key);
    HelpTarget {
        content: content.into(),
        key,
        text: text.into(),
        radius: RadiusSource::Fixed(Radius::default()),
        caption: Caption::Outside,
        accent: theme.accent.into(),
        text_color: theme.text.normal.into(),
        text_bg: theme.solid_bg.into(),
        offline_color: theme.colors.offline.into(),
        active: state.active,
        is_current,
        on_hover,
    }
}

impl<'a, K, Message, Theme, Renderer> HelpTarget<'a, K, Message, Theme, Renderer> {
    pub fn caption(mut self, caption: Caption) -> Self {
        self.caption = caption;
        self
    }

    /// Corner radius of the wrapped element.
    pub fn radius(mut self, radius: impl Into<Radius>) -> Self {
        self.radius = RadiusSource::Fixed(radius.into());
        self
    }

    /// Corner radius taken from the same style function the element uses,
    /// e.g. `.radius_with(|t| bubble_style(t).border.radius)`.
    pub fn radius_with(mut self, f: impl Fn(&Theme) -> Radius + 'a) -> Self {
        self.radius = RadiusSource::Themed(Box::new(f));
        self
    }

    /// Outline and caption border color. Defaults to a built-in accent.
    pub fn accent(mut self, accent: impl Into<Color>) -> Self {
        self.accent = accent.into();
        self
    }

    /// Caption text color. Defaults to white.
    pub fn text_color(mut self, color: impl Into<Color>) -> Self {
        self.text_color = color.into();
        self
    }

    /// Blend target parent outlines desaturate towards. Defaults to the
    /// theme's offline color.
    pub fn offline_color(mut self, color: impl Into<Color>) -> Self {
        self.offline_color = color.into();
        self
    }
}

impl<'a, K: Clone, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for HelpTarget<'a, K, Message, Theme, Renderer>
where
    Renderer: text::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        tree.size = tree.children[0].size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            viewport,
            renderer,
            operation,
        );
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
        // While help mode is active, pointer events are captured before
        // reaching the wrapped content, so hovering/pressing through a
        // target doesn't also trigger the underlying widget's own
        // hover/press behavior (a message's hover toolbar, a server icon's
        // pill animation, ...). This doesn't stop a nested `HelpTarget` from
        // claiming hover itself: that check below doesn't look at the
        // shell's captured flag, only at the cursor position.
        if self.active && matches!(event, Event::Mouse(_) | Event::Touch(_)) {
            shell.capture_event();
        }

        // Children first, so the innermost target claims the hover.
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        if self.active
            && matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. }))
            && cursor.is_over(layout.bounds())
            && !CLAIMED.with(|c| c.replace(true))
            && !self.is_current
        {
            shell.publish((self.on_hover)(Some(self.key.clone())));
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.active {
            return mouse::Interaction::Help;
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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
        let mut overlays = self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        );

        if self.is_current || (self.active && !overlays.is_empty()) {
            // Distance from the current target: 0 for it, 1 for its direct
            // parent, and so on, since deeper levels already pushed their
            // own overlay into `overlays` before we get here.
            let position = overlays.len();
            MAX_POSITION.with(|m| m.set(m.get().max(position)));

            overlays.push(overlay::Element::new(Box::new(HelpOverlay {
                // `translation` accounts for scrollables: both are in window space.
                target: layout.bounds() + translation,
                visible: *viewport + translation,
                radius: &self.radius,
                text: &self.text,
                caption: self.caption,
                accent: self.accent,
                text_color: self.text_color,
                text_bg: self.text_bg,
                offline_color: self.offline_color,
                window,
                // Only the current target (position 0) gets the dim/caption;
                // its parents just get an outline, faded further out. See
                // `HelpOverlay::index` for why parents still draw underneath.
                outline_only: !self.is_current,
                position,
            })));
        }

        overlays
    }
}

impl<'a, K: Clone + 'a, Message: 'a, Theme: 'a, Renderer>
    From<HelpTarget<'a, K, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: text::Renderer + 'a,
{
    fn from(w: HelpTarget<'a, K, Message, Theme, Renderer>) -> Self {
        Element::new(w)
    }
}

// --------------------------------------------------------------- HelpOverlay

struct HelpOverlay<'a, 'b, Theme> {
    target: Rectangle,
    visible: Rectangle,
    radius: &'b RadiusSource<'a, Theme>,
    text: &'b str,
    caption: Caption,
    accent: Color,
    text_color: Color,
    text_bg: Color,
    offline_color: Color,
    window: Size,
    /// Just the outline, no dim/caption -- used for the parents of the
    /// current target, to show nesting without competing with it.
    outline_only: bool,
    /// Distance from the current (innermost) target: 0 for it, 1 for its
    /// direct parent, and so on. Drives both the outline's fade and its
    /// draw order (see `index`).
    position: usize,
}

impl<Message, Theme, Renderer> overlay::Overlay<Message, Theme, Renderer>
    for HelpOverlay<'_, '_, Theme>
where
    Renderer: text::Renderer,
{
    // The default `mouse_interaction` (None) matters: anything else makes iced
    // levitate the cursor for the widgets underneath and hover tracking stops.

    // Higher index draws on top. Default is 1.0 for everything else in the
    // app, so start above that and count down by position so the current
    // target (0) draws above all its parents, which draw above each other
    // in nesting order -- otherwise a parent, built after its child, would
    // render over the current target's outline and caption.
    fn index(&self) -> f32 {
        1000.0 - self.position as f32
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        _cursor: mouse::Cursor,
    ) {
        // Overlays don't get their own layer on master; without one, base text
        // renders above our quads.
        let screen = Rectangle::with_size(self.window);
        renderer.with_layer(screen, |renderer| self.draw_layer(renderer, theme, screen));
    }
}

fn grow(r: Radius, by: f32) -> Radius {
    // Keep square corners square; round corners get a concentric radius.
    let g = |c: f32| if c > 0.0 { c + by } else { 0.0 };
    Radius {
        top_left: g(r.top_left),
        top_right: g(r.top_right),
        bottom_right: g(r.bottom_right),
        bottom_left: g(r.bottom_left),
    }
}

impl<Theme> HelpOverlay<'_, '_, Theme> {
    fn draw_layer<Renderer: text::Renderer>(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        screen: Rectangle,
    ) {
        let base = match self.radius {
            RadiusSource::Fixed(r) => *r,
            RadiusSource::Themed(f) => f(theme),
        };

        // Outline rect, clipped to the visible part of any enclosing scrollable
        // (plus GAP, so content flush with the scrollable edge isn't clipped).
        // Corners on a side that is actually scrolled out become square.
        let full = self.target;
        let clip = self.visible.intersection(&screen).unwrap_or(screen);
        let Some(hole) = full.intersection(&clip) else {
            return;
        };
        let mut radius = grow(base, BORDER_WIDTH);
        let eps = 0.5;
        if hole.y > full.y + eps {
            radius.top_left = 0.0;
            radius.top_right = 0.0;
        }
        if hole.x > full.x + eps {
            radius.top_left = 0.0;
            radius.bottom_left = 0.0;
        }
        if hole.x + hole.width < full.x + full.width - eps {
            radius.top_right = 0.0;
            radius.bottom_right = 0.0;
        }
        if hole.y + hole.height < full.y + full.height - eps {
            radius.bottom_left = 0.0;
            radius.bottom_right = 0.0;
        }

        // Dim with a rounded hole: one quad whose border is so thick it covers
        // the window. The inner edge of a border has radius `outer - width`,
        // so an outer radius of `hole_radius + W` leaves exactly the hole.
        // Skipped for parents of the current target: only the current
        // target's hole should dim the rest of the window.
        if !self.outline_only {
            let w = screen.width + screen.height;
            let outer = Radius {
                top_left: radius.top_left + w,
                top_right: radius.top_right + w,
                bottom_right: radius.bottom_right + w,
                bottom_left: radius.bottom_left + w,
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: hole.expand(w),
                    border: Border {
                        color: DIM,
                        width: w,
                        radius: outer,
                    },
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
        }

        // Outline, desaturated by distance from the current target (blended
        // towards `offline_color`, not faded in opacity): fully saturated
        // at position 0 always (regardless of how deep the chain is, even
        // if there's no chain at all), down to `BASE_OUTLINE_SATURATION` at
        // the outermost ancestor (`max_depth`), scaling everything in
        // between the same way -- clamped to 1.0, since the ratio exceeds
        // it for any position short of the outermost.
        let saturation = if self.position == 0 {
            1.0
        } else {
            let max_depth = MAX_POSITION.with(|m| m.get());
            let fade = (max_depth as f32 + 1.0) / (self.position as f32 + 1.0);
            (BASE_OUTLINE_SATURATION * fade).min(1.0)
        };
        let outline_color = blend(self.offline_color, self.accent, saturation);
        renderer.fill_quad(
            renderer::Quad {
                bounds: hole,
                border: Border {
                    color: outline_color,
                    width: BORDER_WIDTH,
                    radius,
                },
                ..Default::default()
            },
            Color::TRANSPARENT,
        );

        if self.outline_only {
            return;
        }

        // Caption.
        let pad = 8.0;
        let measure = |max_w: f32| {
            let caption = Text {
                content: self.text,
                bounds: Size::new((max_w - BORDER_WIDTH * pad).max(0.0), screen.height),
                size: Pixels(14.0),
                line_height: text::LineHeight::default(),
                font: renderer.font(),
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::Word,
                ellipsis: text::Ellipsis::default(),
                hint_factor: None,
            };
            // Only used for measuring: `fill_paragraph` with a paragraph that is
            // dropped before the frame renders draws nothing, so we use `fill_text`.
            let size = Renderer::Paragraph::with_text(caption).min_bounds();
            (
                caption,
                size,
                Size::new(size.width + 2.0 * pad, size.height + 2.0 * pad),
            )
        };

        // Inside: measure against the element's own width, then check the
        // wrapped result also fits vertically. Otherwise fall back to outside.
        let margin = 12.0;
        let inside = (self.caption == Caption::Inside)
            .then(|| {
                let max_w = (hole.width - 2.0 * margin).min(320.0);
                let (caption, text_size, box_size) = measure(max_w);
                let fits = max_w > 0.0
                    && box_size.width <= hole.width - 2.0 * margin
                    && box_size.height <= hole.height - 2.0 * margin;
                fits.then(|| {
                    let at = Point::new(
                        hole.center_x() - box_size.width / 2.0,
                        hole.center_y() - box_size.height / 2.0,
                    );
                    (caption, text_size, Rectangle::new(at, box_size))
                })
            })
            .flatten();

        let (caption, text_size, label) = inside.unwrap_or_else(|| {
            // Outside: below the element, or above if there's no room below.
            // If there's no room on either side (a full-height element), go
            // beside it instead, on whichever side faces the screen's
            // center. Clamped to the window throughout.
            let (caption, text_size, box_size) = measure((screen.width - 2.0 * pad).min(320.0));
            let (sx2, sy2) = (screen.width, screen.height);

            let below = hole.y + hole.height + pad;
            let fits_below = below + box_size.height <= sy2;
            let above = hole.y - pad - box_size.height;
            let fits_above = above >= pad;

            let at = if fits_below || fits_above {
                let y = if fits_below { below } else { above.max(pad) };
                let x = (hole.center_x() - box_size.width / 2.0)
                    .clamp(pad, (sx2 - pad - box_size.width).max(pad));
                Point::new(x, y)
            } else {
                let x = if hole.center_x() < sx2 / 2.0 {
                    hole.x + hole.width + pad
                } else {
                    hole.x - pad - box_size.width
                }
                .clamp(pad, (sx2 - pad - box_size.width).max(pad));
                let y = (hole.center_y() - box_size.height / 2.0)
                    .clamp(pad, (sy2 - pad - box_size.height).max(pad));
                Point::new(x, y)
            };

            (caption, text_size, Rectangle::new(at, box_size))
        });
        let (x, y) = (label.x, label.y);

        renderer.fill_quad(
            renderer::Quad {
                bounds: label,
                border: Border {
                    color: self.accent,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            },
            self.text_bg,
        );
        renderer.fill_text(
            Text {
                bounds: text_size,
                ..caption.with_content(self.text.to_owned())
            },
            Point::new(x + pad, y + pad),
            self.text_color,
            screen,
        );
    }
}

// ------------------------------------------------------------------ HelpRoot

/// Wrap your whole view in this. While help mode is on it swallows clicks and
/// keys, exits on click or Escape, clears the hover when the cursor is over no
/// target, and dims the window when nothing is hovered.
pub struct HelpRoot<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    active: bool,
    has_hover: bool,
    on_clear: Message,
    on_exit: Message,
}

pub fn help_root<'a, K, Message, Theme, Renderer>(
    state: &HelpState<K>,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    on_hover: fn(Option<K>) -> Message,
    on_exit: Message,
) -> HelpRoot<'a, Message, Theme, Renderer> {
    HelpRoot {
        content: content.into(),
        active: state.active,
        has_hover: state.hovered.is_some(),
        on_clear: on_hover(None),
        on_exit,
    }
}

impl<'a, Message: Clone, Theme, Renderer> Widget<Message, Theme, Renderer>
    for HelpRoot<'a, Message, Theme, Renderer>
where
    Renderer: text::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        tree.size = tree.children[0].size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            viewport,
            renderer,
            operation,
        );
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
        if self.active {
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. }) => {
                    shell.publish(self.on_exit.clone());
                    shell.capture_event();
                    return;
                }
                Event::Mouse(mouse::Event::ButtonReleased(_))
                | Event::Touch(_)
                | Event::Keyboard(keyboard::Event::KeyReleased { .. }) => {
                    shell.capture_event();
                    return;
                }
                Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                    if *key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                        shell.publish(self.on_exit.clone());
                    }
                    shell.capture_event();
                    return;
                }
                // Same trick as `settings`/`quick_select`'s backdrop, which
                // uses a full-window `mouse_area` to levitate the cursor for
                // everything underneath: capture the remaining mouse events
                // (scroll, cursor moves, ...) too, so content that isn't
                // wrapped in a `help()` target -- and so doesn't capture for
                // itself -- doesn't react underneath help mode either. Still
                // forwarded to content below, since `HelpTarget::update`'s
                // own hover-claiming doesn't look at the captured flag.
                Event::Mouse(_) => {
                    shell.capture_event();
                }
                _ => {}
            }
        }

        let is_move = matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. }));
        if is_move {
            CLAIMED.with(|c| c.set(false));
        }

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        if self.active && is_move && !CLAIMED.with(|c| c.get()) && self.has_hover {
            shell.publish(self.on_clear.clone());
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );

        if self.active && !self.has_hover {
            // New layer, otherwise text in the same layer renders above the quad.
            renderer.with_layer(*viewport, |r| {
                r.fill_quad(
                    renderer::Quad {
                        bounds: layout.bounds(),
                        ..Default::default()
                    },
                    DIM,
                );
            });
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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
        // Reset before the whole subtree rebuilds its overlays this frame,
        // so `HelpOverlay::draw_layer` sees the correct max depth once the
        // outermost ancestor in the (single, app-wide) active chain has run.
        MAX_POSITION.with(|m| m.set(0));

        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Message: Clone + 'a, Theme: 'a, Renderer> From<HelpRoot<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: text::Renderer + 'a,
{
    fn from(w: HelpRoot<'a, Message, Theme, Renderer>) -> Self {
        Element::new(w)
    }
}

pub struct HelpView<T> {
    help_state: HelpState<HelpKey>,
    theme: deplace_core::theme::Theme,
    on_hover: fn(Option<HelpKey>) -> T,
}

impl<T: Clone> HelpView<T> {
    pub fn new(
        help_state: HelpState<HelpKey>,
        theme: deplace_core::theme::Theme,
        on_hover: fn(Option<HelpKey>) -> T,
    ) -> Self {
        Self {
            help_state,
            theme,
            on_hover,
        }
    }

    // Generic per call, unlike a closure: lets this be used with a
    // different `text`/`content` type on each call.
    pub fn call<'a, S: Into<Cow<'a, str>>, E: Into<Element<'a, T>>>(
        &self,
        key: HelpKey,
        text: S,
        content: E,
    ) -> HelpTarget<'a, HelpKey, T, iced::Theme, iced::Renderer> {
        help(
            self.help_state,
            self.theme,
            key,
            text,
            content,
            self.on_hover,
        )
    }
}
