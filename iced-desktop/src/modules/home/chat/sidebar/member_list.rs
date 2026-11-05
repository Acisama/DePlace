use std::collections::HashMap;

use deplace_core::member::room::RoomMembers;
use iced::{Alignment, Length, widget::svg};
use macros::iced_cache;

use crate::{
    common::*,
    components::{
        PushElement, help_mode::ChatSidebarHelpKey, phosphor_icon, profile::MemberRenderExt,
        track_bounds::track_bounds,
    },
    modules::home::MIC_ICON,
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
    MemberHovered(OwnedUserId),
    MemberUnhovered(OwnedUserId),
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
    members: Arc<RoomMembers>,

    #[hash]
    hovered_member: Option<OwnedUserId>,

    member_bounds: HashMap<OwnedUserId, Rectangle>,

    #[hash]
    room_id: OwnedRoomId,
}

impl ExtraHash for MemberList {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.room.hash_selective(state, false, true, false);
    }
}

impl MemberList {
    pub fn new(room: &DePlaceRoom) -> Self {
        Self {
            room: room.clone(),

            members: room.members(),
            hovered_member: None,

            member_bounds: HashMap::new(),
            room_id: room.room_id().to_owned(),
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
            MemberListMessage::MemberHovered(user_id) => {
                self.hovered_member = Some(user_id);
                None
            }
            MemberListMessage::MemberUnhovered(user_id) => {
                if self.hovered_member == Some(user_id) {
                    self.hovered_member = None;
                }
                None
            }
        }
    }

    fn view(&self) -> iced::Element<'static, MemberListMessage> {
        let theme = theme();
        let structure = structure();

        let online_members = self.members.online();
        let offline_members = self.members.offline();

        let mut column_children: Vec<Element<'static, MemberListMessage>> = Vec::new();
        let help_view = HelpView::new(theme);

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
                                let is_hovered = self.hovered_member.as_ref() == Some(&id);

                                let bg_color = if is_hovered {
                                    theme.solid_hover_bg.into()
                                } else {
                                    theme.solid_bg.into()
                                };

                                track_bounds(
                                    w::mouse_area(
                                        w::container(
                                            w::row![
                                                member.render_icon_with_presence(
                                                    icon_size, theme, structure, bg_color,
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
                                        .style(move |_| {
                                            ContainerStyle {
                                                background: is_hovered
                                                    .then_some(theme.solid_hover_bg.into()),
                                                border: Border {
                                                    color: if is_hovered {
                                                        theme.border.into()
                                                    } else {
                                                        Color::TRANSPARENT
                                                    },
                                                    width: structure.border_thickness,
                                                    radius: structure.inner_border_radius.into(),
                                                },
                                                ..Default::default()
                                            }
                                        }),
                                    )
                                    .on_enter(MemberListMessage::MemberHovered(id.clone()))
                                    .on_exit(MemberListMessage::MemberUnhovered(id.clone()))
                                    .on_press(
                                        if let Some(bounds) = bounds {
                                            MemberListMessage::MemberPressed {
                                                member: member.clone(),
                                                bounds,
                                            }
                                        } else {
                                            MemberListMessage::None
                                        },
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

        let live = self.room.live();

        let mut row = render_member_numbers(
            online_members.len(),
            offline_members.len(),
            theme,
            structure,
            &help_view,
            true,
        );

        if live.has_call_participants() {
            let length = live.call_participants().len();

            row = row.push_element(themed_tooltip_text(
                help_view
                    .call(
                        ChatSidebarHelpKey::ActiveCallMemberCount,
                        "How many members are in an active call in this room",
                        w::row![
                            phosphor_icon(MIC_ICON, structure.semi_large_font_size() * 0.9)
                                .color(theme.colors.offline.into()),
                            w::text(live.call_participants_count())
                                .size(structure.semi_large_font_size())
                                .color(theme.colors.offline)
                        ]
                        .spacing(structure.small_gap)
                        .padding(padding::horizontal(structure.gap / 2.0))
                        .align_y(Alignment::Center),
                    )
                    .radius(structure.inner_border_radius),
                format!("{} active call members", length),
                structure,
                theme,
            ));
        }

        let header = w::container(row).width(Fill).center_x(Length::Fill);

        if !online_members.is_empty() {
            let length = online_members.len().to_string();

            expand_column(
                format!("Online — {}", length),
                online_members,
                ChatSidebarHelpKey::OnlineMembers,
                "List of online members in the current room",
                &mut column_children,
            );
        }

        let length = offline_members.len().to_string();

        if !offline_members.is_empty() {
            expand_column(
                format!("Offline — {}", length),
                offline_members,
                ChatSidebarHelpKey::OfflineMembers,
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
) -> w::Row<Element<'static, T>> {
    let mut heading_children = Vec::new();

    let icon = move |text,
                     icon,
                     color,
                     key,
                     help,
                     description: String,
                     heading: &mut Vec<Element<'static, T>>| {
        heading.push(
            themed_tooltip_text(
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
                    .radius(structure.inner_border_radius),
                description.clone(),
                structure,
                theme,
            )
            .boxed(),
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
            format!("{} online in this room", online_members),
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
        format!("{} total room members", offline_members),
        &mut heading_children,
    );

    w::Row::with_children(heading_children).align_y(Alignment::Center)
}
