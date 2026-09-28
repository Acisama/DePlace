use std::collections::{BTreeSet, HashMap};

use deplace_core::state::PresenceMap;
use iced::{Alignment, Length, widget::svg};
use macros::iced_cache;
use matrix_sdk::{
    room::RoomMember,
    ruma::{events::presence::PresenceEventContent, presence::PresenceState},
};

use crate::{
    common::*,
    components::{render_presence, track_bounds::track_bounds},
};

#[derive(Debug, Clone)]
pub enum MemberListMessage {
    None,
    Bounds {
        user_id: OwnedUserId,
        bounds: Rectangle,
    },
    MemberPressed {
        member: RoomMember,
        bounds: Rectangle,
    },
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for MemberListMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatar(uri)
    }
}

pub enum MemberListAction {
    NeedsMedia(NeedsMedia),
    ShowProfile {
        member: RoomMember,
        bounds: Rectangle,
    },
}

#[iced_cache(Debug, Clone)]
pub struct MemberList {
    state: AppState,

    room_id: OwnedRoomId,

    membership_map: Receiver<MembershipMap>,
    presence_map: Receiver<PresenceMap>,

    #[hash]
    hovered_member: Option<OwnedUserId>,

    member_bounds: HashMap<OwnedUserId, Rectangle>,

    avatar_cache: AvatarCache,
}

impl MemberList {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        Self {
            state: state.clone(),

            room_id: room.room_id().to_owned(),

            membership_map: state.membership_map(),
            presence_map: state.presence_map(),

            hovered_member: None,

            avatar_cache: state.avatar_cache().clone(),

            member_bounds: HashMap::new(),

            avatar_states_for_hash: BTreeSet::new(),
        }
    }
}

impl IcedWidget<MemberListMessage, MemberListAction> for MemberList {
    fn update(&mut self, message: MemberListMessage) -> Option<MemberListAction> {
        match message {
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
            MemberListMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(MemberListAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, MemberListMessage> {
        let presence_map = self.presence_map.borrow();
        let (online_members, offline_members): (Vec<_>, Vec<_>) = self
            .membership_map
            .borrow()
            .get(&self.room_id)
            .cloned()
            .unwrap_or_default()
            .values()
            .map(|member| {
                (
                    presence_map
                        .get(member.user_id())
                        .cloned()
                        .unwrap_or(PresenceEventContent::new(PresenceState::Offline)),
                    member.clone(),
                )
            })
            .partition(|(presence, _)| !matches!(presence.presence, PresenceState::Offline));

        let mut heading_children: Vec<Element<'static, MemberListMessage>> = Vec::new();
        let mut column_children: Vec<Element<'static, MemberListMessage>> = Vec::new();

        let icon = |text, icon, color, heading: &mut Vec<Element<'static, MemberListMessage>>| {
            heading.push(
                svg(iced::advanced::svg::Handle::from_memory(icon))
                    .width(structure.large_font_size * 0.6)
                    .height(structure.large_font_size * 0.6)
                    .style(move |_, _| w::svg::Style { color: Some(color) })
                    .into(),
            );
            heading.push(
                w::text(text)
                    .size(structure.large_font_size)
                    .color(color)
                    .into(),
            );
        };

        let icon_size = structure.sidebar.dm_icon_height;
        let avatar_cache = &self.avatar_cache;

        let expand_column =
            |text,
             members: Vec<(PresenceEventContent, RoomMember)>,
             column: &mut Vec<Element<'static, MemberListMessage>>| {
                column.push(
                    w::column![
                        w::text(text).color(theme.text.dim),
                        Space::new().height(structure.small_gap / 4.0)
                    ]
                    .extend(members.iter().map(|(p, member)| {
                        let id = member.user_id().to_owned();
                        let bounds = self.member_bounds.get(&id).cloned();

                        track_bounds(
                            w::button(
                                w::row![
                                    render_presence(
                                        member,
                                        &p.presence,
                                        theme,
                                        structure,
                                        icon_size,
                                        avatar_cache,
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
                                background: status.active().then_some(theme.solid_hover_bg.into()),
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
                            .on_press_maybe(bounds.map(|bounds| {
                                MemberListMessage::MemberPressed {
                                    member: member.clone(),
                                    bounds,
                                }
                            })),
                            move |bounds| MemberListMessage::Bounds {
                                user_id: id.clone(),
                                bounds,
                            },
                        )
                        .into()
                    }))
                    .padding(structure.divider_width)
                    .into(),
                );
            };

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
                &mut heading_children,
            );

            expand_column(
                format!("Online — {}", length),
                online_members,
                &mut column_children,
            );
        }

        if !offline_members.is_empty() {
            let length = offline_members.len().to_string();

            icon(
                length.clone(),
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/indicators/offline.svg"
                ))
                .to_vec(),
                theme.colors.offline.into(),
                &mut heading_children,
            );

            expand_column(
                format!("Offline — {}", length),
                offline_members,
                &mut column_children,
            );
        }

        w::column![
            w::container(
                w::Row::with_children(heading_children.into_iter())
                    .spacing(structure.gap)
                    .align_y(Alignment::Center)
            )
            .width(Fill)
            .center_x(Length::Fill)
        ]
        .extend(column_children)
        .spacing(structure.gap)
        .width(Fill)
        .height(Fill)
        .padding(structure.gap)
        .into()
    }
}
