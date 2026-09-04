use std::hash::Hash;

use deplace_core::ProfileLike;
use deplace_core::state::cache::{AvatarCache, MediaState};
use iced::advanced::svg::Renderer as SvgRenderer;
use iced::advanced::{Widget, layout};
use iced::font::Weight;
use iced::widget::canvas::{Frame, Path, Stroke};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::text::Rich;
use iced::widget::{self as w, Canvas, canvas, image, rich_text, span, svg};
use iced::{
    Border, Element,
    widget::{Container, Stack},
};
use iced::{Color, ContentFit, Font, Point, Renderer, Size};
use matrix_sdk::Room;
use matrix_sdk::ruma::OwnedMxcUri;
use matrix_sdk::ruma::serde::Base64;

use crate::things::Structure;
use crate::things::{Colors, Theme};

pub(crate) mod authentification;
pub(crate) mod home;
mod on_appear;
pub(crate) mod overlay;
pub(crate) mod root;
pub(crate) mod shader;

pub use on_appear::on_appear;

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
    text: impl Into<String>,
    weight: iced::font::Weight,
) -> Rich<'a, (), T> {
    rich_text([span(text.into()).font(Font {
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

pub fn floating_tile<'a, T>(
    theme: Theme,
    structure: Structure,
    content: impl Into<Element<'a, T>>,
) -> Container<'a, T> {
    use w::container::Style;
    w::container(content).style(move |_theme| Style {
        background: Some(theme.background.into()),
        border: Border {
            color: theme.border,
            width: structure.border_thickness,
            radius: structure.outer_border_radius.into(),
        },
        ..Style::default()
    })
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
            background: theme.background.into(),
            border: Border {
                color: if matches!(status, Status::Focused { .. }) {
                    theme.accent
                } else {
                    theme.border
                },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            placeholder: theme.text.muted,
            selection: theme.text.muted,
            value: theme.text.normal,
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

pub fn text_icon<'a, T: 'a>(
    text: impl Into<String> + iced::advanced::text::IntoFragment<'a>,
    size: f32,
    rounding: f32,
    color: Color,
) -> Element<'a, T> {
    Stack::new()
        .push(
            w::container(weighted_text(text, Weight::Bold).size(size / 2.0))
                .width(size)
                .height(size)
                .center(size)
                .style(move |_| w::container::Style {
                    background: Some(color.scale_lightness(0.2).into()),
                    text_color: Some(color),
                    border: Border {
                        color,
                        width: 0.0,
                        radius: rounding.into(),
                    },
                    ..Default::default()
                }),
        )
        .push(
            Canvas::new(InsetShadow::new(rounding, color, size / 8.0, 8))
                .width(size)
                .height(size),
        )
        .into()
}

pub fn unknown_icon<'a, T: 'a>(size: f32, rounding: f32, theme: Theme) -> Element<'a, T> {
    text_icon('?', size, rounding, theme.colors.error)
}

pub fn loading_icon<'a, T: 'a>(size: f32, rounding: f32, theme: Theme) -> Element<'a, T> {
    text_icon('#', size, rounding, theme.colors.offline)
}

pub fn render_avatar<'a, T: NeedsAvatarExt + Clone + 'a>(
    uri: Option<OwnedMxcUri>,
    size: f32,
    rounding: f32,
    fallback: impl FnOnce() -> Element<'a, T>,
    avatar_cache: &AvatarCache,
) -> Element<'a, T> {
    let Some(avatar_url) = uri else {
        return fallback();
    };

    match avatar_cache.get(&avatar_url) {
        Some(MediaState::Failed) | Some(MediaState::Loading) => fallback(),
        Some(MediaState::Loaded(avatar)) => image((*avatar).clone())
            .width(size)
            .height(size)
            .content_fit(ContentFit::Cover)
            .border_radius(rounding)
            .into(),
        None => on_appear(fallback(), T::needs_avatar(avatar_url)).into(),
    }
}

/// A trait for messages which have a NeedAvatar variant
pub trait NeedsAvatarExt {
    fn needs_avatar(uri: OwnedMxcUri) -> Self;
}

pub trait ProfileRenderExt {
    fn render_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
    ) -> Element<'a, T>;

    fn render_name<'a, T: Clone + 'a>(&self, size: f32) -> Element<'a, T>;
}

impl<Profile: ProfileLike> ProfileRenderExt for Profile {
    fn render_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
    ) -> Element<'a, T> {
        let rounding = size * Self::ICON_BORDER_RADIUS_RATIO;

        let fallback = move || text_icon(self.initial(), size, rounding, self.color().to_iced());

        render_avatar(
            self.profile_avatar(),
            size,
            rounding,
            fallback,
            avatar_cache,
        )
    }

    fn render_name<'a, T: Clone + 'a>(&self, size: f32) -> Element<'a, T> {
        render_name(self.get_name(), size, self.color().to_iced())
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
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &R,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        layout::Node::new(limits.resolve(self.size, self.size, Size::new(self.size, self.size)))
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut R,
        _theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: layout::Layout<'_>,
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

pub fn context_room_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
    room: &Room,
    size: f32,
    avatar_cache: &AvatarCache,
) -> Element<'a, T> {
    if room.is_dm() {
        return room.render_icon(size, avatar_cache);
    }

    phosphor_icon(
        if room.is_call() {
            phosphor_svgs::icon::speaker_high::FILL
        } else {
            phosphor_svgs::icon::hash::BOLD
        },
        size,
    )
    .into()
}

pub trait IcedWidget<T, V> {
    fn update(&mut self, message: T) -> Option<V>;
    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, T>;
}

pub fn render_name<'a, T: Clone + 'a>(name: String, size: f32, color: Color) -> Element<'a, T> {
    weighted_text(name, Weight::Bold)
        .size(size)
        .color(color)
        .into()
}

pub fn render_unknown_name<'a, T: Clone + 'a>(size: f32, theme: Theme) -> Element<'a, T> {
    render_name("Unknown".to_string(), size, theme.colors.error)
}

pub fn render_loading_name<'a, T: Clone + 'a>(size: f32, theme: Theme) -> Element<'a, T> {
    render_name("Loading...".to_string(), size, theme.colors.offline)
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
