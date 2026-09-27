use std::hash::Hash;

use corner_badge::{notch_circle, positioned};
use deplace_core::ProfileLike;
use deplace_core::rooms::DePlaceRoom;
use deplace_core::settings::NameDecoration;
use deplace_core::state::PresenceMap;
use deplace_core::state::cache::{AvatarCache, MediaState};
use deplace_core::structure::Structure;
use deplace_core::theme::{Colors, Theme};
use iced::advanced::svg::Renderer as SvgRenderer;
use iced::advanced::{Widget, layout};
use iced::alignment::{Horizontal, Vertical};
use iced::font::Weight;
use iced::widget::canvas::{Frame, Path, Stroke};
use iced::widget::image::Handle as ImageHandle;
use iced::widget::text::{IntoFragment, LineHeight, Rich};
use iced::widget::{
    self as w, Canvas, Scrollable, Space, canvas, image, responsive, rich_text, span, svg,
};
use iced::{
    Alignment, Color, ContentFit, Fill, Font, Length, Padding, Point, Renderer, Size, padding,
};
use iced::{Border, Element, widget::Stack};
use matrix_sdk::room::RoomMember;
use matrix_sdk::ruma::OwnedMxcUri;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::serde::Base64;
use tile_background::TileBackground;

pub(crate) mod animation_clock;
pub mod authentification;
pub mod corner_badge;
pub mod home;
pub mod on_appear;
pub mod root;
pub mod shader;
pub mod tile_background;
pub mod track_bounds;
pub mod track_scroll;

pub use corner_badge::{CornerContent, corner_badge};
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
    text_icon('?', size, rounding, theme.colors.error.into())
}

pub fn loading_icon<'a, T: 'a>(size: f32, rounding: f32, theme: Theme) -> Element<'a, T> {
    text_icon('#', size, rounding, theme.colors.offline.into())
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

    fn render_name_decorated<'a, T: Clone + 'a>(
        &self,
        size: f32,
        decoration: NameDecoration,
        text_color: Color,
    ) -> Element<'a, T>;
}

impl<Profile: ProfileLike> ProfileRenderExt for Profile {
    /// Renders the profile's avatar icon.
    fn render_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
    ) -> Element<'a, T> {
        let rounding = size * self.icon_border_radius_ratio();

        let fallback = move || text_icon(self.initial(), size, rounding, self.color().to_iced());

        render_avatar(self.get_avatar(), size, rounding, fallback, avatar_cache)
    }

    /// Renders the profile's name in bold using the profile's color.
    fn render_name<'a, T: Clone + 'a>(&self, size: f32) -> Element<'a, T> {
        render_name(self.get_name(), size, self.color().to_iced())
    }

    /// Renders the profile's name with the normal text color, but decorated with the profile's color.
    fn render_name_decorated<'a, T: Clone + 'a>(
        &self,
        size: f32,
        decoration: NameDecoration,
        text_color: Color,
    ) -> Element<'a, T> {
        render_name_decorated(
            self.get_name(),
            size,
            decoration,
            text_color,
            self.color().into(),
        )
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

pub fn context_room_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
    room: &DePlaceRoom,
    icon_size: f32,
    theme: Theme,
    structure: Structure,
    presence_map: &PresenceMap,
    avatar_cache: &AvatarCache,
    background_color: Color,
) -> Element<'a, T> {
    if room.is_dm() {
        return render_room_with_presence_map(
            room,
            presence_map,
            theme,
            structure,
            icon_size,
            avatar_cache,
            background_color,
        );
    }

    phosphor_icon(room.icon(), icon_size).into()
}

pub trait IcedWidget<T, V> {
    fn update(&mut self, message: T) -> Option<V>;
    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, T>;
}

pub fn render_name<'a, T: Clone + 'a>(name: String, size: f32, color: Color) -> Element<'a, T> {
    weighted_text(name, Weight::Bold)
        .size(size)
        .color(color)
        .line_height(LineHeight::Relative(1.0))
        .align_y(Alignment::End)
        .into()
}

pub fn render_unknown_name<'a, T: Clone + 'a>(size: f32, theme: Theme) -> Element<'a, T> {
    render_name("Unknown".to_string(), size, theme.colors.error.into())
}

pub fn render_loading_name<'a, T: Clone + 'a>(size: f32, theme: Theme) -> Element<'a, T> {
    render_name("Loading...".to_string(), size, theme.colors.offline.into())
}

pub fn blend_colors(color_a: Color, color_b: Color, factor: f32) -> Color {
    let t = factor.clamp(0.0, 1.0);

    Color {
        r: color_a.r + (color_b.r - color_a.r) * t,
        g: color_a.g + (color_b.g - color_a.g) * t,
        b: color_a.b + (color_b.b - color_a.b) * t,
        a: color_a.a + (color_b.a - color_a.a) * t,
    }
}

pub fn render_name_decorated<'a, T: Clone + 'a>(
    name: String,
    size: f32,
    decoration: NameDecoration,
    text_color: Color,
    decoration_color: Color,
) -> Element<'a, T> {
    match decoration {
        NameDecoration::None => w::text(name).size(size).color(text_color).into(),
        NameDecoration::FirstLetter => {
            let mut chars = name.chars();
            let Some((first, rest)) = chars.next().map(|first| (first, chars.collect::<String>()))
            else {
                return Space::new().into();
            };

            w::row![
                w::text(first).size(size).color(decoration_color),
                w::text(rest).size(size).color(text_color)
            ]
            .into()
        }
        NameDecoration::Gradient => {
            let chars = name.chars();
            let length = name.len();

            w::Row::with_children(chars.enumerate().map(|(i, c)| {
                let factor = i as f32 / length as f32;

                w::text(c)
                    .color(blend_colors(decoration_color, text_color, factor))
                    .size(size)
                    .into()
            }))
            .into()
        }
    }
}

pub fn render_unknown_name_decorated<'a, T: Clone + 'a>(
    size: f32,
    decoration: NameDecoration,
    text_color: Color,
    theme: Theme,
) -> Element<'a, T> {
    render_name_decorated(
        "Unknown".to_string(),
        size,
        decoration,
        text_color,
        theme.colors.error.into(),
    )
}

pub fn render_loading_name_decorated<'a, T: Clone + 'a>(
    size: f32,
    decoration: NameDecoration,
    text_color: Color,
    theme: Theme,
) -> Element<'a, T> {
    render_name_decorated(
        "Loading...".to_string(),
        size,
        decoration,
        text_color,
        theme.colors.offline.into(),
    )
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

pub fn themed_tooltip<'a, T: 'a>(
    content: impl Into<Element<'a, T>>,
    tooltip: impl IntoFragment<'a>,
    structure: Structure,
    theme: Theme,
) -> w::tooltip::Tooltip<'a, T> {
    w::tooltip(
        content,
        w::container(w::text(tooltip).color(theme.text.normal))
            .padding(padding::horizontal(structure.small_gap).vertical(structure.small_gap / 2.0)),
        w::tooltip::Position::Bottom,
    )
    .delay(std::time::Duration::from_millis(300))
    .style(move |_| w::container::Style {
        background: Some(theme.solid_bg.into()),
        border: Border {
            color: theme.border.into(),
            width: structure.border_thickness,
            radius: structure.inner_border_radius.into(),
        },
        text_color: Some(theme.text.normal.into()),
        ..Default::default()
    })
}

pub fn render_presence<'a, T: 'a + Clone + NeedsAvatarExt>(
    member: &RoomMember,
    presence: &PresenceState,
    theme: Theme,
    structure: Structure,
    icon_size: f32,
    avatar_cache: &AvatarCache,
    background_color: Color,
) -> Element<'a, T> {
    let (color, icon) = match presence {
        matrix_sdk::ruma::presence::PresenceState::Offline => (
            theme.colors.offline.into(),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/indicators/offline.svg"
            ))
            .to_vec(),
        ),
        matrix_sdk::ruma::presence::PresenceState::Unavailable => (
            theme.colors.idle.into(),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/indicators/idle.svg"
            ))
            .to_vec(),
        ),
        _ => (
            theme.colors.online.into(),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/indicators/online.svg"
            ))
            .to_vec(),
        ),
    };

    let ratio = 0.25;
    let bg_circle_size = structure.icon_gap;

    Stack::new()
        .push(member.render_icon(icon_size, avatar_cache))
        .push(responsive(move |size| {
            let base_dim = size.width.min(size.height);
            let notch_diameter = base_dim * (ratio + bg_circle_size);
            let content_diameter = base_dim * ratio;
            let overflow = base_dim * bg_circle_size / 2.0;

            let h = Horizontal::Right;
            let v = Vertical::Bottom;

            let notch = notch_circle(notch_diameter, background_color);

            Stack::new()
                .push(positioned(notch, h, v, overflow))
                .push(positioned(
                    svg(iced::advanced::svg::Handle::from_memory(icon.clone()))
                        .width(content_diameter)
                        .height(content_diameter)
                        .style(move |_, _| w::svg::Style { color: Some(color) })
                        .into(),
                    h,
                    v,
                    0.0,
                ))
        }))
        .into()
}

pub fn render_presence_with_map<'a, T: 'a + Clone + NeedsAvatarExt>(
    member: &RoomMember,
    presence_map: &PresenceMap,
    theme: Theme,
    structure: Structure,
    icon_size: f32,
    avatar_cache: &AvatarCache,
    background_color: Color,
) -> Element<'a, T> {
    let presence = presence_map
        .get(member.user_id())
        .map(|p| p.presence.clone())
        .unwrap_or(matrix_sdk::ruma::presence::PresenceState::Offline);

    render_presence(
        member,
        &presence,
        theme,
        structure,
        icon_size,
        avatar_cache,
        background_color,
    )
}

pub fn render_room_with_presence_map<'a, T: 'a + Clone + NeedsAvatarExt>(
    room: &DePlaceRoom,
    presence_map: &PresenceMap,
    theme: Theme,
    structure: Structure,
    icon_size: f32,
    avatar_cache: &AvatarCache,
    background_color: iced::Color,
) -> Element<'a, T> {
    if room.is_dm()
        && let Some(other_member) = room.dm_other_member()
    {
        render_presence_with_map(
            &other_member,
            presence_map,
            theme,
            structure,
            icon_size,
            avatar_cache,
            background_color,
        )
    } else {
        room.render_icon(icon_size, avatar_cache)
    }
}
