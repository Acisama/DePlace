use std::collections::BTreeSet;

use deplace_core::state::PresenceMap;
use iced::{Alignment, Length, widget::svg};
use macros::iced_cache;
use matrix_sdk::ruma::{events::presence::PresenceEventContent, presence::PresenceState};

use crate::common::*;

#[derive(Debug, Clone)]
pub enum MemberListMessage {
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for MemberListMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatar(uri)
    }
}

pub enum MemberListAction {
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Debug, Clone)]
pub struct MemberList {
    state: AppState,

    room_id: OwnedRoomId,

    membership_map: Receiver<MembershipMap>,
    presence_map: Receiver<PresenceMap>,

    avatar_cache: AvatarCache,
}

impl MemberList {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        Self {
            state: state.clone(),

            room_id: room.room_id().to_owned(),

            membership_map: state.membership_map(),
            presence_map: state.presence_map(),

            avatar_cache: state.avatar_cache().clone(),

            avatar_states_for_hash: BTreeSet::new(),
        }
    }
}

impl IcedWidget<MemberListMessage, MemberListAction> for MemberList {
    fn update(&mut self, message: MemberListMessage) -> Option<MemberListAction> {
        match message {
            MemberListMessage::NeedsAvatar(uri) => {
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

        if !online_members.is_empty() {
            icon(
                online_members.len().to_string(),
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/indicators/online.svg"
                ))
                .to_vec(),
                theme.colors.online.into(),
                &mut heading_children,
            );
        }

        if !offline_members.is_empty() {
            icon(
                offline_members.len().to_string(),
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/indicators/offline.svg"
                ))
                .to_vec(),
                theme.colors.offline.into(),
                &mut heading_children,
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
        .width(Fill)
        .height(Fill)
        .padding(structure.gap)
        .into()
    }
}
