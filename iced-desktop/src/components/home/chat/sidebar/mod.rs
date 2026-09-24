use std::collections::BTreeSet;

use enumset::{EnumSet, EnumSetType};
use macros::iced_cache;
use matrix_sdk::room::RoomMember;

use crate::common::*;

#[derive(Debug, EnumSetType)]
enum SidebarState {
    Member,
    MemberList,
    Pins,
    Search,
}

impl SidebarState {
    fn width(&self, structure: &Structure) -> f32 {
        match self {
            SidebarState::Member => structure.chat_sidebar_width.member,
            SidebarState::MemberList => structure.chat_sidebar_width.member_list,
            SidebarState::Pins => structure.chat_sidebar_width.pinned,
            SidebarState::Search => structure.chat_sidebar_width.search,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SidebarMessage {
    NeedsAvatarUrl(OwnedMxcUri),
}

impl NeedsAvatarExt for SidebarMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatarUrl(uri)
    }
}

pub enum SidebarAction {
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone, Debug)]
pub struct Sidebar {
    state: EnumSet<SidebarState>,

    room: DePlaceRoom,
    avatar_cache: AvatarCache,

    member: Option<RoomMember>,
}

impl Sidebar {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        Self {
            state: EnumSet::new(),
            room: room.clone(),
            member: None,

            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),
        }
    }

    pub fn set_member(&mut self, member: RoomMember) {
        self.state.insert(SidebarState::Member);
        self.member = Some(member);
    }

    pub fn toggle_member_list(&mut self) {
        if self.state.contains(SidebarState::MemberList) {
            self.state.remove(SidebarState::MemberList);
        } else {
            self.state.insert(SidebarState::MemberList);
        }
    }

    pub fn toggle_pins(&mut self) {
        if self.state.contains(SidebarState::Pins) {
            self.state.remove(SidebarState::Pins);
        } else {
            self.state.insert(SidebarState::Pins);
        }
    }

    pub fn toggle_search(&mut self) {
        if self.state.contains(SidebarState::Search) {
            self.state.remove(SidebarState::Search);
        } else {
            self.state.insert(SidebarState::Search);
        }
    }

    fn get_currently_visible(&self) -> Option<SidebarState> {
        [
            SidebarState::Member,
            SidebarState::Search,
            SidebarState::Pins,
            SidebarState::MemberList,
        ]
        .into_iter()
        .find(|&state| self.state.contains(state))
    }
}

impl IcedWidget<SidebarMessage, SidebarAction> for Sidebar {
    fn update(&mut self, message: SidebarMessage) -> Option<SidebarAction> {
        match message {
            SidebarMessage::NeedsAvatarUrl(url) => {
                self.avatar_states_for_hash.insert(url.clone());
                Some(SidebarAction::NeedsMedia(NeedsMedia::avatar(url)))
            }
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, SidebarMessage> {
        let Some(state) = self.get_currently_visible() else {
            return w::space().into();
        };

        floating_tile(
            theme,
            structure,
            w::container("test")
                .height(Fill)
                .width(state.width(&structure)),
        )
        .into()
    }
}
