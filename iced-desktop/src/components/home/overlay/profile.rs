use std::collections::BTreeSet;

use deplace_core::state::PresenceMap;
use iced::Alignment;
use macros::iced_cache;

use crate::{
    common::*,
    components::{CopyUserIdExt, render_banner_column},
};

#[derive(Debug, Clone)]
pub enum ProfileMessage {
    NeedsAvatar(OwnedMxcUri),
    CopyUserId(OwnedUserId),
    UserIdCopied,
}

impl NeedsAvatarExt for ProfileMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatar(uri)
    }
}

impl CopyUserIdExt for ProfileMessage {
    fn copy_user_id(id: OwnedUserId) -> Self {
        Self::CopyUserId(id)
    }
}

pub enum ProfileAction {
    NeedsMedia(NeedsMedia),
    CopyUserId(OwnedUserId),
}

#[iced_cache(Clone, Debug)]
pub struct OverlayProfile {
    state: AppState,

    member: RoomMember,

    bounds: Rectangle,

    avatar_cache: AvatarCache,
    presence_map: Receiver<PresenceMap>,
}

impl OverlayProfile {
    pub fn new(state: &AppState, member: RoomMember, bounds: Rectangle) -> Self {
        Self {
            state: state.clone(),

            member,
            bounds,

            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),

            presence_map: state.presence_map().clone(),
        }
    }
}

impl IcedWidget<ProfileMessage, ProfileAction> for OverlayProfile {
    fn update(&mut self, message: ProfileMessage) -> Option<ProfileAction> {
        match message {
            ProfileMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(ProfileAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
            ProfileMessage::CopyUserId(id) => Some(ProfileAction::CopyUserId(id)),
            ProfileMessage::UserIdCopied => None,
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, ProfileMessage> {
        let bounds = self.bounds;
        let member = self.member.clone();
        let presence_map = self.presence_map.borrow().clone();
        let avatar_cache = self.avatar_cache.clone();

        w::responsive(move |size| {
            let content: Element<'static, ProfileMessage> = floating_tile(
                theme,
                structure,
                w::container(render_banner_column(
                    &member,
                    &presence_map,
                    &avatar_cache,
                    theme,
                    structure,
                ))
                .style(move |_| ContainerStyle {
                    background: Some(theme.solid_bg.into()),
                    border: Border {
                        color: theme.border.into(),
                        width: structure.border_thickness,
                        radius: structure.outer_border_radius.into(),
                    },
                    ..Default::default()
                })
                .padding(padding::bottom(structure.gap)),
            )
            .into();

            let gap = structure.small_gap;
            let popup = w::container(content).padding(gap);

            if bounds.center_x() < size.width / 2.0 {
                // Bounds are left of center, so open toward the center (right).
                w::container(popup)
                    .width(Fill)
                    .height(Fill)
                    .padding(Padding {
                        top: bounds.y,
                        left: bounds.x + bounds.width + gap,
                        right: 0.0,
                        bottom: 0.0,
                    })
            } else {
                // Bounds are right of center, so open toward the center (left).
                w::container(popup)
                    .width(bounds.x - gap)
                    .height(Fill)
                    .align_x(Alignment::End)
                    .padding(Padding {
                        top: bounds.y,
                        ..Default::default()
                    })
            }
        })
        .into()
    }
}
