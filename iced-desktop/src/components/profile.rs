use deplace_core::{
    DePlaceRoom, ProfileLike,
    settings::NameDecoration,
    state::{
        PresenceMap,
        cache::{AvatarCache, MediaState},
    },
    structure::Structure,
    theme::Theme,
};
use iced::{
    Alignment, Border, Color, ContentFit, Element,
    Length::Fill,
    Rectangle,
    alignment::{Horizontal, Vertical},
    border::{self, Radius},
    font::Weight,
    never, padding,
    widget::{
        self as w, Canvas, Space, Stack, responsive, svg,
        text::{IntoFragment, LineHeight},
    },
};
use matrix_sdk::{
    room::RoomMember,
    ruma::{OwnedMxcUri, OwnedRoomId, OwnedUserId, presence::PresenceState},
};

use super::{
    CopyUserIdExt, IcedColorExt, InsetShadow, StatusExt,
    corner_badge::{notch_circle, positioned},
    link, on_appear, phosphor_icon, weighted_text,
};

pub fn text_icon<'a, T: 'a>(
    text: impl IntoFragment<'a>,
    size: f32,
    rounding: f32,
    color: Color,
) -> Element<'a, T> {
    Stack::new()
        .push(
            w::container(weighted_text(text, Weight::Bold).size((size / 2.0).max(1.0)))
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
        Some(MediaState::Loaded(avatar)) => w::image((*avatar).clone())
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

pub trait OpenProfileOverlayExt: Sized {
    fn open_profile(room_id: OwnedRoomId, user_id: OwnedUserId, bounds: Rectangle) -> Self;
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

pub trait MaybeRenderIcon {
    fn render_icon<'a, T: Clone + NeedsAvatarExt + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
        rounding_factor: f32,
        theme: Theme,
    ) -> Element<'a, T>;
}

impl MaybeRenderIcon for Option<&RoomMember> {
    fn render_icon<'a, T: Clone + NeedsAvatarExt + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
        rounding_factor: f32,
        theme: Theme,
    ) -> Element<'a, T> {
        match self {
            Some(member) => member.render_icon(size, avatar_cache),
            None => unknown_icon(size, size * rounding_factor, theme),
        }
    }
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

pub fn render_profile_name_with_overlay<
    'a,
    P: ProfileLike + Clone + 'a,
    T: Clone + OpenProfileOverlayExt + 'a,
>(
    profile: &P,
    room_id: OwnedRoomId,
    user_id: OwnedUserId,
    size: f32,
) -> Element<'a, T> {
    let profile = profile.clone();
    let color = profile.color().into();

    link::link(
        render_name(profile.get_name(), size, color),
        color,
        move |bounds| T::open_profile(room_id.clone(), user_id.clone(), bounds),
    )
    .into()
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
        NameDecoration::FullColor => w::text(name).size(size).color(decoration_color).into(),
        NameDecoration::Gradient => {
            let chars = name.chars();
            let length = name.len();

            w::text::Rich::with_spans(
                chars
                    .enumerate()
                    .map(|(i, c)| {
                        let factor = i as f32 / length as f32;

                        iced::advanced::text::Span::new(c)
                            .size(size)
                            .color(blend_colors(decoration_color, text_color, factor))
                    })
                    .collect::<Vec<_>>(),
            )
            .on_link_click(never)
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

pub fn render_banner_column<'a, T: Clone + CopyUserIdExt + NeedsAvatarExt + 'a>(
    member: &RoomMember,
    presence_map: &PresenceMap,
    avatar_cache: &AvatarCache,
    theme: Theme,
    structure: Structure,
) -> Element<'a, T> {
    let color = member.color().to_iced();
    let sidebar = structure.chat.sidebar;

    let icon_size = sidebar.large_icon_size;
    let icon_gap = structure.icon_gap * icon_size;
    let bg_icon_size = icon_size + icon_gap;

    let id = member.user_id().to_owned();

    w::stack([
        w::column![
            w::container("")
                .style(move |_| w::container::Style {
                    background: Some(color.into()),
                    border: border::rounded(Radius {
                        top_left: structure.outer_border_radius,
                        top_right: structure.outer_border_radius,
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .width(Fill)
                .height(sidebar.banner_height),
            Space::new().height(icon_size / 2.0),
            w::column![
                member.render_name(structure.large_font_size),
                Space::new().height(structure.small_gap),
                w::button(
                    w::text(id.to_string())
                        .color(theme.text.dim)
                        .size(structure.font_size)
                )
                .padding(0.0)
                .on_press(T::copy_user_id(id))
                .style(move |_, status| w::button::Style {
                    background: None,
                    text_color: if status.active() {
                        theme.text.normal.into()
                    } else {
                        theme.text.dim.into()
                    },
                    ..Default::default()
                })
            ]
            .padding(padding::left(structure.small_gap * 2.0))
        ]
        .into(),
        w::row![
            Space::new().width(structure.small_gap * 2.0 - icon_gap / 2.0),
            w::column![
                Space::new().height(sidebar.banner_height - 2.0 / 3.0 * bg_icon_size),
                w::stack([
                    w::container(
                        w::container("")
                            .width(bg_icon_size)
                            .height(bg_icon_size)
                            .style(move |_| w::container::Style {
                                background: Some(theme.solid_bg.into()),
                                border: border::rounded(bg_icon_size / 2.0),
                                ..Default::default()
                            })
                    )
                    .into(),
                    w::row![
                        Space::new().width(icon_gap / 2.0),
                        w::column![
                            Space::new().height(icon_gap / 2.0),
                            render_presence_with_map(
                                member,
                                presence_map,
                                theme,
                                structure,
                                icon_size,
                                avatar_cache,
                                theme.solid_bg.into()
                            )
                        ]
                    ]
                    .into()
                ])
                .width(Fill)
            ]
            .width(Fill)
        ]
        .into(),
    ])
    .width(sidebar.width.member)
    .into()
}
