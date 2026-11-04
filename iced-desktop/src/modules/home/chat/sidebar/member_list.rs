use std::collections::HashMap;

use iced::{Alignment, Length, widget::svg};
use macros::iced_cache;

use crate::{
    common::*,
    components::{
        help_mode::ChatSidebarHelpKey, profile::MemberRenderExt, track_bounds::track_bounds,
    },
};

#[derive(Debug, Clone)]
pub enum MemberListMessage {
    None,
    Bounds {
        user_id: OwnedUserId,
        bounds: Rectangle,
    },
    MemberPressed {
        member: Member,
        bounds: Rectangle,
    },
    HelpHover(Option<HelpKey>),
}

impl HelpOverMessage for MemberListMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        MemberListMessage::HelpHover(help)
    }
}

pub enum MemberListAction {
    ShowProfile { member: Member, bounds: Rectangle },
    HelpHover(Option<HelpKey>),
}

#[iced_cache(Debug, Clone)]
pub struct MemberList {
    room: DePlaceRoom,

    #[hash]
    hovered_member: Option<OwnedUserId>,

    member_bounds: HashMap<OwnedUserId, Rectangle>,
}

impl MemberList {
    pub fn new(room: &DePlaceRoom) -> Self {
        Self {
            room: room.clone(),

            hovered_member: None,

            member_bounds: HashMap::new(),
        }
    }
}

impl IcedWidget<MemberListMessage, MemberListAction> for MemberList {
    fn update(&mut self, message: MemberListMessage) -> Option<MemberListAction> {
        match message {
            MemberListMessage::HelpHover(help_key) => Some(MemberListAction::HelpHover(help_key)),
            MemberListMessage::None => None,
            MemberListMessage::Bounds { user_id, bounds } => {
                self.member_bounds.insert(user_id, bounds);
                None
            }
            MemberListMessage::MemberPressed { member, bounds } => {
                tracing::trace!(
                    "Member {:?} pressed at bounds {:?}",
                    member.user_id(),
                    bounds
                );
                Some(MemberListAction::ShowProfile { member, bounds })
            }
        }
    }

    fn view(&self) -> iced::Element<'static, MemberListMessage> {
        let theme = theme();
        let structure = structure();

        let members = self.room.members();

        let online_members = members.online();
        let offline_members = members.offline();

        let mut heading_children: Vec<Element<'static, MemberListMessage>> = Vec::new();
        let mut column_children: Vec<Element<'static, MemberListMessage>> = Vec::new();

        let help_view = HelpView::new(theme);

        let icon = |text,
                    icon,
                    color,
                    key,
                    help,
                    heading: &mut Vec<Element<'static, MemberListMessage>>| {
            heading.push(
                help_view
                    .call(
                        key,
                        help,
                        w::row![
                            svg(iced::advanced::svg::Handle::from_memory(icon))
                                .width(structure.semi_large_font_size() * 0.6)
                                .height(structure.semi_large_font_size() * 0.6)
                                .style(move |_, _| w::svg::Style { color: Some(color) }),
                            w::text(text)
                                .size(structure.semi_large_font_size())
                                .color(color)
                        ]
                        .spacing(structure.small_gap)
                        .padding(padding::horizontal(structure.gap / 2.0))
                        .align_y(Alignment::Center),
                    )
                    .radius(structure.inner_border_radius)
                    .into(),
            );
        };

        let icon_size = structure.sidebar.dm_icon_height;

        let expand_column =
            |text,
             members: Arc<Vec<Member>>,
             key,
             help,
             column: &mut Vec<Element<'static, MemberListMessage>>| {
                column.push(
                    help_view
                        .call(
                            key,
                            help,
                            w::column![
                                w::text(text).color(theme.text.dim),
                                Space::new().height(structure.small_gap / 4.0)
                            ]
                            .extend(members.iter().map(|member| {
                                let id = member.user_id().to_owned();
                                let bounds = self.member_bounds.get(&id).cloned();

                                track_bounds(
                                    w::button(
                                        w::row![
                                            member.render_icon_with_presence(
                                                icon_size,
                                                theme,
                                                structure,
                                                theme.solid_bg.into(),
                                            ),
                                            member.render_name(structure.font_size)
                                        ]
                                        .padding(
                                            padding::vertical(structure.small_gap * 0.75)
                                                .horizontal(structure.small_gap),
                                        )
                                        .width(Fill)
                                        .spacing(structure.gap)
                                        .align_y(Alignment::Center),
                                    )
                                    .padding(0.0)
                                    .style(move |_, status| ButtonStyle {
                                        background: status
                                            .active()
                                            .then_some(theme.solid_hover_bg.into()),
                                        border: Border {
                                            color: if status.active() {
                                                theme.border.into()
                                            } else {
                                                Color::TRANSPARENT
                                            },
                                            width: structure.border_thickness,
                                            radius: structure.inner_border_radius.into(),
                                        },
                                        ..Default::default()
                                    })
                                    .on_press_maybe(
                                        bounds.map(|bounds| MemberListMessage::MemberPressed {
                                            member: member.clone(),
                                            bounds,
                                        }),
                                    ),
                                    move |bounds| MemberListMessage::Bounds {
                                        user_id: id.clone(),
                                        bounds,
                                    },
                                )
                                .into()
                            }))
                            .padding(structure.divider_width),
                        )
                        .radius(structure.inner_border_radius)
                        .into(),
                );
            };

        let header = w::container(render_member_numbers(
            online_members.len(),
            offline_members.len(),
            theme,
            structure,
            &help_view,
            true,
        ))
        .width(Fill)
        .center_x(Length::Fill);

        if !online_members.is_empty() {
            let length = online_members.len().to_string();

            icon(
                length.clone(),
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/indicators/online.svg"
                ))
                .to_vec(),
                theme.colors.online.into(),
                HelpKey::ChatSidebar(ChatSidebarHelpKey::OnlineMemberCount),
                "How many online members are in the current room",
                &mut heading_children,
            );

            expand_column(
                format!("Online — {}", length),
                online_members,
                HelpKey::ChatSidebar(ChatSidebarHelpKey::OnlineMembers),
                "List of online members in the current room",
                &mut column_children,
            );
        }

        let length = offline_members.len().to_string();

        icon(
            length.clone(),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/indicators/offline.svg"
            ))
            .to_vec(),
            theme.colors.offline.into(),
            HelpKey::ChatSidebar(ChatSidebarHelpKey::OfflineMemberCount),
            "How many offline members are in the current room",
            &mut heading_children,
        );

        if !offline_members.is_empty() {
            expand_column(
                format!("Offline — {}", length),
                offline_members,
                HelpKey::ChatSidebar(ChatSidebarHelpKey::OfflineMembers),
                "List of offline members in the current room",
                &mut column_children,
            );
        }

        w::column![header]
            .extend(column_children)
            .spacing(structure.gap)
            .width(Fill)
            .height(Fill)
            .padding(structure.gap)
            .boxed()
    }
}

pub fn render_member_numbers<T: Clone + HelpOverMessage + 'static>(
    online_members: usize,
    offline_members: usize,
    theme: Theme,
    structure: Structure,
    help_view: &HelpView<T>,
    big: bool,
) -> Element<'static, T> {
    let mut heading_children = Vec::new();

    let icon = |text, icon, color, key, help, heading: &mut Vec<Element<'static, T>>| {
        heading.push(
            help_view
                .call(
                    key,
                    help,
                    w::row![
                        svg(iced::advanced::svg::Handle::from_memory(icon))
                            .width(structure.semi_large_font_size() * 0.6)
                            .height(structure.semi_large_font_size() * 0.6)
                            .style(move |_, _| w::svg::Style { color: Some(color) }),
                        w::text(text)
                            .size(if big {
                                structure.semi_large_font_size()
                            } else {
                                structure.font_size
                            })
                            .color(color)
                    ]
                    .spacing(structure.small_gap)
                    .padding(padding::horizontal(structure.gap / 2.0))
                    .align_y(Alignment::Center),
                )
                .radius(structure.inner_border_radius)
                .into(),
        );
    };

    if online_members > 0 {
        icon(
            online_members.to_string(),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/indicators/online.svg"
            ))
            .to_vec(),
            theme.colors.online.into(),
            HelpKey::ChatSidebar(ChatSidebarHelpKey::OnlineMemberCount),
            "How many online members are in the current room",
            &mut heading_children,
        );
    }

    icon(
        (offline_members + online_members).to_string(),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/indicators/offline.svg"
        ))
        .to_vec(),
        theme.colors.offline.into(),
        HelpKey::ChatSidebar(ChatSidebarHelpKey::OfflineMemberCount),
        "How many offline members are in the current room",
        &mut heading_children,
    );

    w::Row::with_children(heading_children)
        .align_y(Alignment::Center)
        .boxed()
}
