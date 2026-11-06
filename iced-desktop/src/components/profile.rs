use deplace_core::{
    Avatar, DePlaceRoom, ProfileLike,
    member::{Member, user_data::Presence},
    room::DePlaceRoomType,
    settings::NameDecoration,
    structure::Structure,
    theme::Theme,
};

use iced::{
    Alignment, Border, Color, ContentFit, Element,
    Length::Fill,
    Rectangle, Widget,
    alignment::{Horizontal, Vertical},
    border::{self, Radius},
    font::Weight,
    never, padding,
    widget::{
        self as w, Canvas, Space, Stack, responsive, svg,
        text::{IntoFragment, LineHeight},
    },
};

use crate::{
    common::{structure, theme, themed_tooltip_content},
    modules::home::{DEFAULT_ROOM_ICON, SPACE_ICON, SPEAKER_ICON},
};

use super::{
    ColoredRounded, CopyUserIdExt, IcedColorExt, InsetShadow, PushElement, StatusExt,
    corner_badge::{notch_circle, positioned},
    link, phosphor_icon, weighted_text,
};

pub fn text_icon<'a, T: 'a>(
    text: impl IntoFragment<'a>,
    size: f32,
    rounding: f32,
    color: Color,
) -> Element<'a, T> {
    w::stack![
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
        Canvas::new(InsetShadow::new(rounding, color, size / 8.0, 8))
            .width(size)
            .height(size),
    ]
    .boxed()
}

pub fn unknown_icon<'a, T: 'a>(size: f32, rounding: f32, theme: Theme) -> Element<'a, T> {
    text_icon('?', size, rounding, theme.colors.error.into())
}

pub fn loading_icon<'a, T: 'a>(size: f32, rounding: f32, theme: Theme) -> Element<'a, T> {
    text_icon('#', size, rounding, theme.colors.offline.into())
}

pub fn render_avatar<'a, T: 'a>(
    size: f32,
    rounding: f32,
    fallback: impl FnOnce() -> Element<'a, T>,
    avatar: Avatar,
) -> Element<'a, T> {
    match avatar {
        Avatar::Ok { image, .. } => w::image(image)
            .width(size)
            .height(size)
            .content_fit(ContentFit::Cover)
            .border_radius(rounding)
            .boxed(),
        Avatar::Error(error) => {
            let theme = theme();
            let structure = structure();

            themed_tooltip_content(
                fallback(),
                w::text((*error).clone())
                    .color(theme.colors.error)
                    .size(structure.font_size),
                structure,
                theme,
                w::tooltip::Position::Top,
                0.0,
            )
            .boxed()
        }
        Avatar::NoAvatar => fallback(),
    }
}

pub trait OpenProfileOverlayExt: Sized {
    fn open_profile(member: Member, bounds: Rectangle) -> Self;
}

pub trait ProfileRenderExt {
    /// Renders the profile's avatar icon.
    fn render_icon<'a, T: 'a>(&self, size: f32) -> Element<'a, T>;

    /// Renders the profile's small avatar icon.
    fn render_small_icon<'a, T: 'a>(&self, size: f32) -> Element<'a, T>;

    /// Renders the profile's name in bold using the profile's color.
    fn render_name<'a, T: 'a>(&self, size: f32) -> Element<'a, T>;

    /// Renders the profile's name with the normal text color, but decorated with the profile's color.
    fn render_name_decorated<'a, T: 'a>(
        &self,
        size: f32,
        decoration: NameDecoration,
        text_color: Color,
    ) -> Element<'a, T>;
}

pub trait MemberRenderExt {
    /// Renders the profile's avatar icon with the user's presence indicator.
    fn render_icon_with_presence<'a, T: 'a>(
        &self,
        size: f32,
        theme: Theme,
        structure: Structure,
        background_color: Color,
    ) -> Element<'a, T>;

    /// Render the name, but open the profile on click.
    fn render_name_with_overlay<'a, T: OpenProfileOverlayExt + 'a>(
        &self,
        text_size: f32,
    ) -> Element<'a, T>;
}

pub trait RoomRenderExt {
    fn render_simple_icon<'a, T: 'a>(
        &self,
        size: f32,
        theme: Theme,
        structure: Structure,
        background_color: Color,
    ) -> Element<'a, T>;
}

impl<Profile: ProfileLike> ProfileRenderExt for Profile {
    fn render_icon<'a, T: 'a>(&self, size: f32) -> Element<'a, T> {
        let rounding = size * self.icon_border_radius_ratio();

        let fallback = move || text_icon(self.initial(), size, rounding, self.color().to_iced());

        render_avatar(size, rounding, fallback, self.avatar_handle())
    }

    fn render_small_icon<'a, T: 'a>(&self, size: f32) -> Element<'a, T> {
        let rounding = size * self.icon_border_radius_ratio();

        let fallback = move || text_icon(self.initial(), size, rounding, self.color().to_iced());

        render_avatar(size, rounding, fallback, self.small_avatar_handle())
    }

    fn render_name<'a, T: 'a>(&self, size: f32) -> Element<'a, T> {
        render_name(self.displayname(), size, self.color().to_iced())
    }

    fn render_name_decorated<'a, T: 'a>(
        &self,
        size: f32,
        decoration: NameDecoration,
        text_color: Color,
    ) -> Element<'a, T> {
        render_name_decorated(
            self.displayname(),
            size,
            decoration,
            text_color,
            self.color().into(),
        )
    }
}

impl MemberRenderExt for Member {
    fn render_icon_with_presence<'a, T: 'a>(
        &self,
        size: f32,
        theme: Theme,
        structure: Structure,
        background_color: Color,
    ) -> Element<'a, T> {
        render_presence_badge(
            self.render_icon(size),
            self.presence(),
            theme,
            structure,
            background_color,
        )
    }

    fn render_name_with_overlay<'a, T: OpenProfileOverlayExt + 'a>(
        &self,
        text_size: f32,
    ) -> Element<'a, T> {
        let member = self.clone();

        link::link(
            self.render_name(text_size),
            self.name_color().to_iced(),
            move |bounds| T::open_profile(member.clone(), bounds),
        )
        .into()
    }
}

impl RoomRenderExt for DePlaceRoom {
    fn render_simple_icon<'a, T: 'a>(
        &self,
        size: f32,
        theme: Theme,
        structure: Structure,
        background_color: Color,
    ) -> Element<'a, T> {
        if let Some(member) = self.other_dm_member() {
            return member.render_icon_with_presence(size, theme, structure, background_color);
        }

        match self.room_type() {
            DePlaceRoomType::Call => {
                let icon = phosphor_icon(SPEAKER_ICON, size);

                if self.has_call() {
                    icon.color(theme.colors.success.into()).into()
                } else {
                    icon.into()
                }
            }
            DePlaceRoomType::Space => phosphor_icon(SPACE_ICON, size).into(),
            DePlaceRoomType::Standard | DePlaceRoomType::Direct => {
                phosphor_icon(DEFAULT_ROOM_ICON, size).into()
            }
        }
    }
}

pub fn render_name<'a, T: 'a>(name: String, size: f32, color: Color) -> Element<'a, T> {
    weighted_text(name, Weight::Bold)
        .size(size)
        .color(color)
        .line_height(LineHeight::Relative(1.0))
        .align_y(Alignment::End)
        .boxed()
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

pub fn render_name_decorated<'a, T: 'a>(
    name: String,
    size: f32,
    decoration: NameDecoration,
    text_color: Color,
    decoration_color: Color,
) -> Element<'a, T> {
    match decoration {
        NameDecoration::None => w::text(name).size(size).color(text_color).boxed(),
        NameDecoration::FirstLetter => {
            let mut chars = name.chars();
            let Some((first, rest)) = chars.next().map(|first| (first, chars.collect::<String>()))
            else {
                return Space::new().boxed();
            };

            w::row![
                w::text(first).size(size).color(decoration_color),
                w::text(rest).size(size).color(text_color)
            ]
            .boxed()
        }
        NameDecoration::FullColor => w::text(name).size(size).color(decoration_color).boxed(),
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
            .boxed()
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

pub fn render_presence_badge<'a, T: 'a>(
    icon: Element<'a, T>,
    presence: Presence,
    theme: Theme,
    structure: Structure,
    background_color: Color,
) -> Element<'a, T> {
    let (color, badge) = match presence {
        Presence::Offline => (
            theme.colors.offline.into(),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/indicators/offline.svg"
            ))
            .to_vec(),
        ),
        Presence::Unavailable => (
            theme.colors.idle.into(),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/indicators/idle.svg"
            ))
            .to_vec(),
        ),
        Presence::Online => (
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

    w::stack![
        icon,
        responsive(move |size| {
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
                    svg(iced::advanced::svg::Handle::from_memory(badge.clone()))
                        .width(content_diameter)
                        .height(content_diameter)
                        .style(move |_, _| w::svg::Style { color: Some(color) })
                        .boxed(),
                    h,
                    v,
                    0.0,
                ))
        })
    ]
    .boxed()
}

pub fn render_banner_column<'a, T: CopyUserIdExt + Clone + 'a>(
    member: &Member,
    theme: Theme,
    structure: Structure,
) -> Element<'a, T> {
    let color = member.name_color().to_iced();
    let sidebar = structure.chat.sidebar;

    let icon_size = sidebar.large_icon_size;
    let icon_gap = structure.icon_gap * icon_size;
    let bg_icon_size = icon_size + icon_gap;

    let id = member.user_id().to_owned();

    w::stack![
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
        ],
        w::row![
            Space::new().width(structure.small_gap * 2.0 - icon_gap / 2.0),
            w::column![
                Space::new().height(sidebar.banner_height - 2.0 / 3.0 * bg_icon_size),
                w::stack![
                    w::container(
                        w::container("")
                            .width(bg_icon_size)
                            .height(bg_icon_size)
                            .style(move |_| w::container::Style {
                                background: Some(theme.solid_bg.into()),
                                border: border::rounded(bg_icon_size / 2.0),
                                ..Default::default()
                            })
                    ),
                    w::row![
                        Space::new().width(icon_gap / 2.0),
                        w::column![
                            Space::new().height(icon_gap / 2.0),
                            member.render_icon_with_presence(
                                icon_size,
                                theme,
                                structure,
                                theme.solid_bg.into()
                            )
                        ]
                    ]
                ]
                .width(Fill)
            ]
            .width(Fill)
        ]
    ]
    .width(sidebar.width.member)
    .boxed()
}

const MAX_ICONS: usize = 5;

pub fn render_row_of_icons<'a, T: 'a, Profile: ProfileLike>(
    profiles: &[Profile],
    background_color: Color,
    icon_size: f32,
    structure: Structure,
) -> Element<'a, T> {
    let (profiles, overflow) = if profiles.len() > MAX_ICONS {
        (
            profiles
                .iter()
                .take(MAX_ICONS - 1)
                .collect::<Vec<&Profile>>(),
            Some(profiles.len() - MAX_ICONS),
        )
    } else {
        (profiles.iter().collect::<Vec<&Profile>>(), None)
    };

    let overlap = 1.0 / 3.0;
    let step = icon_size * (1.0 - overlap);
    let get_width = |index: usize| icon_size + index as f32 * step;

    let mut index = 0;
    let mut stack = Stack::new();

    let outer = structure.divider_width;
    let outer_width = icon_size + outer * 2.0;

    for member in profiles.iter() {
        stack = stack.push_element(
            w::container(
                w::container("")
                    .width(outer_width)
                    .height(outer_width)
                    .colored_rounded(background_color, outer_width / 2.0),
            )
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::Center)
            .padding(padding::left(index as f32 * step)),
        );
        stack = stack.push_element(
            w::container(member.render_small_icon(icon_size))
                .width(Fill)
                .height(Fill)
                .align_x(Alignment::Start)
                .align_y(Alignment::Center)
                .padding(padding::left(index as f32 * step + outer)),
        );
        index += 1;
    }

    if let Some(overflow) = overflow {
        stack = stack.push_element(
            w::container(text_icon(
                format!("+{overflow}"),
                icon_size,
                icon_size / 2.0,
                background_color,
            ))
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::Center)
            .padding(padding::left(index as f32 * step + outer)),
        );
        index += 1;
    }

    w::container(stack)
        .width(get_width(index - 1) + outer * 2.0)
        .height(outer_width)
        .center_y(outer_width)
        .boxed()
}
