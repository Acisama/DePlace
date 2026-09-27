use std::collections::BTreeSet;

use crate::{common::*, components::render_presence};
use deplace_core::state::PresenceMap;
use iced::border::Radius;
use macros::iced_cache;
use matrix_sdk::room::RoomMember;

#[derive(Debug, Clone)]
pub enum ProfileCardMessage {
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for ProfileCardMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ProfileCardMessage::NeedsAvatar(uri)
    }
}

pub enum ProfileCardAction {
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone, Debug)]
pub struct ProfileCard {
    state: AppState,

    avatar_cache: AvatarCache,
    presence_map: Receiver<PresenceMap>,

    member: Option<RoomMember>,
}

impl ExtraHash for ProfileCard {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.member
            .as_ref()
            .map(|m| (m.get_avatar(), m.get_name()))
            .hash(state);
    }
}

impl ProfileCard {
    pub fn new(state: &AppState) -> Self {
        Self {
            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),

            presence_map: state.presence_map().clone(),
            state: state.clone(),

            member: None,
        }
    }

    pub fn set_member(&mut self, member: Option<RoomMember>) {
        self.member = member;
    }
}

impl IcedWidget<ProfileCardMessage, ProfileCardAction> for ProfileCard {
    fn update(&mut self, message: ProfileCardMessage) -> Option<ProfileCardAction> {
        match message {
            ProfileCardMessage::NeedsAvatar(uri) => {
                Some(ProfileCardAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, ProfileCardMessage> {
        let sidebar = structure.chat.sidebar;

        let Some(member) = &self.member else {
            return w::container("No other member present").into();
        };

        let color = member.color();

        let icon_size = sidebar.large_icon_size;
        let icon_gap = structure.icon_gap * icon_size;
        let bg_icon_size = icon_size + icon_gap;

        w::stack([
            w::column![
                w::container("")
                    .style(move |_| ContainerStyle {
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
                w::column![member.render_name(structure.large_font_size)]
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
                                .style(move |_| ContainerStyle {
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
                                render_presence(
                                    member,
                                    &self.presence_map.borrow(),
                                    theme,
                                    structure,
                                    icon_size,
                                    &self.avatar_cache,
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
        .height(Fill)
        .width(Fill)
        .into()
    }
}
