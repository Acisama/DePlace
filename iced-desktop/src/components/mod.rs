use std::hash::Hash;

use deplace_core::helpers::DisplayString;
use deplace_core::structure::Structure;
use deplace_core::theme::{Colors, Theme};
use help_mode::HelpState;
use home::HelpKey;
use iced::advanced::svg::Renderer as SvgRenderer;
use iced::advanced::{Widget, layout};
use iced::font::Weight;
use iced::widget::canvas::{Frame, Path, Stroke};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::text::{IntoFragment, Rich};
use iced::widget::{self as w, Canvas, Scrollable, canvas, rich_text, span, svg};
use iced::{Border, Element, widget::Stack};
use iced::{Color, Fill, Font, Length, Padding, Point, Renderer, Size, border};
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::ruma::serde::Base64;
use tile_background::TileBackground;

pub(crate) mod animation_clock;
pub mod authentification;
pub mod corner_badge;
pub mod equal_width;
pub mod help_mode;
pub mod home;
pub mod link;
pub mod on_appear;
pub mod pan;
pub mod profile;
pub mod root;
pub mod shader;
pub mod tile_background;
pub mod tooltip;
pub mod track_bounds;
pub mod track_scroll;

pub use corner_badge::{CornerContent, corner_badge};
pub use on_appear::on_appear;
pub use pan::pan;

pub enum GenericState<T: Clone> {
    Ready,
    Checking,
    Success(T),
    Error(String),
}

impl<T: Clone> Hash for GenericState<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            GenericState::Ready => 0.hash(state),
            GenericState::Checking => 1.hash(state),
            GenericState::Success(_) => 2.hash(state),
            GenericState::Error(e) => e.hash(state),
        }
    }
}

pub fn weighted_text<'a, T>(
    text: impl IntoFragment<'a>,
    weight: iced::font::Weight,
) -> Rich<'a, (), T> {
    rich_text([span(text).font(Font {
        weight,
        ..Default::default()
    })])
}

impl<T: Clone> GenericState<T> {
    pub fn success(&self) -> Option<T> {
        match self {
            GenericState::Success(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self, GenericState::Ready)
    }

    pub fn text<V>(
        &self,
        success: &str,
        ready: &str,
        colors: &Colors,
        structure: &Structure,
    ) -> Rich<'static, (), V> {
        let (text, color) = match self {
            GenericState::Ready => (ready.to_string(), colors.success),
            GenericState::Success(_) => (success.to_string(), colors.success),
            GenericState::Error(e) => (e.clone(), colors.error),
            GenericState::Checking => ("Checking...".to_string(), colors.offline),
        };

        weighted_text(text, iced::font::Weight::Semibold)
            .color(color)
            .size(structure.font_size)
    }
}

/// A card-like surface used throughout the app (chat, sidebars, settings, auth
/// screens, ...), returned by [`floating_tile`].
///
/// Its background is a fake blur: rather than blurring whatever actually sits
/// behind the tile, it samples a shared, heavily-downscaled copy of the app's
/// animated background shader (see [`tile_background`]). That means it never
/// shows blurred *content* - only a positionally-matching patch of the animated
/// background - which is enough to sell the frosted-glass look without a real
/// per-pixel blur pass.
///
/// The background is a separate widget layered behind the content (needed to
/// sample the shared blur texture), so `.padding()`/`.width()`/`.height()`
/// forward to the content layer rather than being native `Stack` methods.
pub struct FloatingTile<'a, T> {
    content: w::Container<'a, T>,
    background: w::Shader<T, TileBackground>,
}

impl<'a, T> FloatingTile<'a, T> {
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.content = self.content.padding(padding);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.content = self.content.width(width);
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.content = self.content.height(height);
        self
    }
}

impl<'a, T> From<FloatingTile<'a, T>> for Element<'a, T>
where
    T: 'a,
{
    fn from(tile: FloatingTile<'a, T>) -> Self {
        Stack::new()
            .push(tile.content)
            .push_under(tile.background)
            .into()
    }
}

pub fn floating_tile<'a, T>(
    theme: Theme,
    structure: Structure,
    content: impl Into<Element<'a, T>>,
) -> FloatingTile<'a, T> {
    FloatingTile {
        content: w::container(content),
        background: w::Shader::new(TileBackground::new(theme, structure))
            .width(Fill)
            .height(Fill),
    }
}

pub fn text_input<T>(
    placeholder: &'static str,
    value: impl Into<String>,
    theme: Theme,
    structure: Structure,
) -> w::text_input::TextInput<'static, T>
where
    T: Clone + 'static,
{
    use w::text_input::{Status, Style};

    w::text_input(placeholder, value.into())
        .size(structure.font_size)
        .padding(structure.small_gap)
        .style(move |_theme, status| Style {
            background: theme.solid_bg.into(),
            border: Border {
                color: if matches!(status, Status::Focused { .. }) {
                    theme.accent.into()
                } else {
                    theme.border.into()
                },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            placeholder: theme.text.muted.into(),
            selection: theme.text.muted.into(),
            value: theme.text.normal.into(),
        })
}

struct InsetShadow {
    radius: f32,
    color: Color,
    depth: f32,
    layers: usize,
}

impl InsetShadow {
    fn new(radius: f32, color: Color, depth: f32, layers: usize) -> Self {
        Self {
            radius,
            color,
            depth,
            layers,
        }
    }
}

impl<Message> canvas::Program<Message> for InsetShadow {
    type State = ();

    fn draw(
        &self,
        _: &Self::State,
        renderer: &Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::advanced::mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());

        for i in 0..self.layers {
            let t = i as f32 / self.layers as f32; // 0.0 at the edge, 1.0 at `depth`
            // Start slightly inside the container's own clipped edge so the stroke
            // (which straddles its path) never bleeds past the boundary and hits the
            // degenerate radius == size/2 corner-arc case, which showed up as flattened
            // poles on the outermost ring.
            let inset = 1.0 + t * self.depth;
            let alpha = self.color.a * (1.0 - t).powf(2.0); // falls off quadratically inward

            let path = Path::rounded_rectangle(
                Point::new(inset, inset),
                Size::new(bounds.width - inset * 2.0, bounds.height - inset * 2.0),
                (self.radius - inset).max(0.0).into(), // shrink the radius as we inset, same as the outer shape
            );

            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(Color {
                        a: alpha,
                        ..self.color
                    })
                    .with_width(self.depth / self.layers as f32 * 1.5),
            );
        }

        vec![frame.into_geometry()]
    }
}

pub trait IcedColorExt {
    fn scale_lightness(&self, factor: f32) -> Self;
}
impl IcedColorExt for iced::Color {
    fn scale_lightness(&self, factor: f32) -> Self {
        iced::Color::from_rgba(self.r * factor, self.g * factor, self.b * factor, self.a)
    }
}

pub struct PhosphorIcon {
    handle: svg::Handle,
    size: f32,
    color: Option<Color>, // explicit override; None = inherit
}

impl PhosphorIcon {
    pub fn new(svg_content: &'static str, size: f32) -> Self {
        Self {
            handle: svg::Handle::from_memory(svg_content.as_bytes()),
            size,
            color: None,
        }
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl<Message, Theme, R: SvgRenderer> Widget<Message, Theme, R> for PhosphorIcon {
    fn size(&self) -> Size<iced::Length> {
        Size::new(self.size.into(), self.size.into())
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        _renderer: &R,
        limits: &iced::advanced::layout::Limits,
    ) {
        tree.size = limits.resolve(self.size, self.size, Size::new(self.size, self.size));
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut R,
        _theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: layout::Layout,
        _cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        renderer.draw_svg(
            iced::advanced::svg::Svg::new(self.handle.clone())
                .color(self.color.unwrap_or(style.text_color)),
            layout.bounds(),
            *viewport,
        );
    }
}

impl<'a, Message, Theme, R> From<PhosphorIcon> for Element<'a, Message, Theme, R>
where
    Message: 'a,
    Theme: 'a,
    R: SvgRenderer + 'a,
{
    fn from(icon: PhosphorIcon) -> Self {
        Element::new(icon)
    }
}

pub fn phosphor_icon(svg_content: &'static str, size: f32) -> PhosphorIcon {
    PhosphorIcon::new(svg_content, size)
}

pub trait IcedWidget<T, V> {
    fn update(&mut self, message: T) -> Option<V>;
    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, T>;
}

pub trait StatusExt {
    fn active(&self) -> bool;
}

impl StatusExt for w::button::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Hovered | Self::Pressed)
    }
}

impl StatusExt for w::text_input::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Focused { .. })
    }
}

impl StatusExt for w::text_editor::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Focused { .. })
    }
}

impl StatusExt for w::scrollable::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Dragged { .. })
    }
}

impl StatusExt for sweeten::widget::toggler::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }
}

pub fn blurhash_to_image(hash: &str) -> Option<ImageHandle> {
    let width = 32;
    let height = 32;

    let pixels = match blurhash::decode(hash, width, height, 1.2) {
        Ok(pixels) => pixels,
        Err(e) => {
            tracing::error!("Failed to decode blurhash: {:?}", e);
            return None;
        }
    };

    Some(ImageHandle::from_rgba(width, height, pixels))
}

pub fn thumbhash_to_image(hash: &Base64) -> Option<ImageHandle> {
    let (width, height, pixels) = match thumbhash::thumb_hash_to_rgba(hash.as_bytes()) {
        Ok(decoded) => decoded,
        Err(e) => {
            tracing::error!("Failed to decode thumbhash: {:?}", e);
            return None;
        }
    };

    Some(ImageHandle::from_rgba(width as u32, height as u32, pixels))
}

pub fn themed_scrollable<'a, T: 'a>(
    content: impl Into<Element<'a, T>>,
    theme: Theme,
    structure: Structure,
) -> Scrollable<'a, T> {
    w::scrollable(content)
        .style(move |_, status| {
            let border = Border {
                color: theme.border.into(),
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            };

            let rail = w::scrollable::Rail {
                background: None,
                border,
                scroller: w::scrollable::Scroller {
                    background: theme.solid_hover_bg.into(),
                    border: border.color(if status.active() {
                        theme.accent
                    } else {
                        theme.border
                    }),
                },
            };

            w::scrollable::Style {
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: None,
                container: w::container::Style::default(),
                auto_scroll: w::scrollable::AutoScroll {
                    background: theme.solid_hover_bg.into(),
                    border,
                    shadow: Default::default(),
                    icon: theme.accent.into(),
                },
            }
        })
        .smooth_scroll(false)
        .auto_scroll(true)
}

pub trait CopyUserIdExt {
    fn copy_user_id(id: OwnedUserId) -> Self;
}

#[derive(Debug, Clone, Copy, Hash, PartialEq)]
pub enum MediaType {
    Image,
    Video,
}

impl DisplayString for MediaType {
    fn display_string(&self) -> String {
        match self {
            MediaType::Image => "Image".to_string(),
            MediaType::Video => "Video".to_string(),
        }
    }
}

pub fn render_media_failed_to_load<'a, T: Clone + 'a>(
    theme: Theme,
    structure: Structure,
    width: f32,
    height: f32,
    kind: MediaType,
) -> Element<'a, T> {
    w::stack![
        w::container(
            weighted_text(
                format!("{} failed to load", kind.display_string()),
                Weight::Bold
            )
            .size(structure.chat.text_size * 1.5)
            .width(Fill)
            .height(Fill)
            .center(),
        )
        .width(width)
        .height(height)
        .style(move |_| w::container::Style {
            background: Some(theme.error_blended().into()),
            text_color: Some(theme.colors.error.into()),
            border: Border {
                color: theme.colors.error.into(),
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }),
        Canvas::new(InsetShadow::new(
            structure.inner_border_radius,
            theme.colors.error.into(),
            structure.chat.text_size / 2.0,
            8,
        ))
        .width(width)
        .height(height),
    ]
    .into()
}

pub trait Explainable<'a, T: Clone + 'a>
where
    Self: Sized,
{
    fn explain(self, color: Color) -> Element<'a, T>;
    fn explain_white(self) -> Element<'a, T> {
        self.explain(Color::WHITE)
    }
}

impl<'a, T: Clone + 'a, E: Into<Element<'a, T>>> Explainable<'a, T> for E {
    fn explain(self, color: Color) -> Element<'a, T> {
        let el = self.into();
        el.explain(color)
    }
}

pub fn close_button<'a, T: Clone + 'a>(
    theme: Theme,
    structure: Structure,
    on_press: T,
) -> Element<'a, T> {
    w::container(
        w::button(phosphor_icon(
            phosphor_svgs::icon::x::BOLD,
            structure.chat.text_size,
        ))
        .padding(structure.small_gap / 2.0)
        .style(move |_, status| w::button::Style {
            background: if status.active() {
                Some(theme.solid_hover_bg.into())
            } else {
                None
            },
            text_color: if status.active() {
                theme.text.normal.into()
            } else {
                theme.text.dim.into()
            },
            border: border::rounded(structure.semi_border_radius()),
            ..Default::default()
        })
        .on_press(on_press),
    )
    .padding(structure.small_gap / 2.0)
    .into()
}
