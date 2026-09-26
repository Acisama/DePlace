use enumset::{EnumSet, EnumSetType};
use macros::iced_cache;
use matrix_sdk::room::RoomMember;
use matrix_sdk_ui::{Timeline, eyeball_im::VectorDiff, timeline::TimelineItem as SdkTimelineItem};
use profilecard::{ProfileCard, ProfileCardAction, ProfileCardMessage};
use sweeten::widget::list;

use crate::{common::*, components::home::chat::timeline::messages::ToTimelineItem};

use super::timeline::messages::TimelineItem;

#[derive(Debug, EnumSetType, Hash)]
pub enum SidebarState {
    Member,
    MemberList,
    Pins,
    Search,
}

mod profilecard;

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
    ProfileCard(ProfileCardMessage),
    PinnedDiffs(Vec<VectorDiff<Arc<SdkTimelineItem>>>),
}

pub enum SidebarAction {
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone, Debug)]
pub struct Sidebar {
    state: AppState,

    #[hash]
    visual_state: EnumSet<SidebarState>,
    #[hash]
    currently_visible: Option<SidebarState>,

    #[hash]
    member_profile_card: ProfileCard,
    #[hash]
    dm_profile_card: ProfileCard,

    room: DePlaceRoom,

    pinned_timeline: Option<Arc<Timeline>>,
    content: list::Content<String, Arc<TimelineItem>>,
    #[hash]
    pinned_messages_version: u64,
}

impl Sidebar {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        Self {
            state: state.clone(),

            visual_state: EnumSet::new(),
            currently_visible: None,

            room: room.clone(),

            pinned_timeline: None,
            content: list::Content::new(),
            pinned_messages_version: 0,

            member_profile_card: ProfileCard::new(state),
            dm_profile_card: ProfileCard::new(state),
        }
    }

    pub fn set_member(&mut self, member: Option<RoomMember>) {
        self.visual_state.insert(SidebarState::Member);
        self.member_profile_card.set_member(member);
        self.calculate_currently_visible();
    }

    pub fn toggle_member_list(&mut self) {
        if self.visual_state.contains(SidebarState::MemberList) {
            self.visual_state.remove(SidebarState::MemberList);
        } else {
            if self.room.is_dm() {
                self.visual_state.insert(SidebarState::MemberList);
                self.dm_profile_card.set_member(self.room.dm_other_member());
            } else {
                self.visual_state.insert(SidebarState::MemberList);
            }
        }
        self.calculate_currently_visible();
    }

    pub fn toggle_pins(&mut self) {
        if self.visual_state.contains(SidebarState::Pins) {
            self.visual_state.remove(SidebarState::Pins);
        } else {
            self.visual_state.insert(SidebarState::Pins);
        }
        self.calculate_currently_visible();
    }

    pub fn toggle_search(&mut self) {
        if self.visual_state.contains(SidebarState::Search) {
            self.visual_state.remove(SidebarState::Search);
        } else {
            self.visual_state.insert(SidebarState::Search);
        }
        self.calculate_currently_visible();
    }

    fn calculate_currently_visible(&mut self) {
        self.currently_visible = [
            SidebarState::Member,
            SidebarState::Search,
            SidebarState::Pins,
            SidebarState::MemberList,
        ]
        .into_iter()
        .find(|&state| self.visual_state.contains(state));
    }

    pub fn load_timeline(
        &mut self,
        pinned_timeline: Arc<Timeline>,
        pinned_initial: Arc<IndexMap<String, Arc<TimelineItem>>>,
    ) {
        self.pinned_timeline = Some(pinned_timeline);
        self.content = list::Content::with_items((*pinned_initial).clone());
    }
}

impl IcedWidget<SidebarMessage, SidebarAction> for Sidebar {
    fn update(&mut self, message: SidebarMessage) -> Option<SidebarAction> {
        match message {
            SidebarMessage::ProfileCard(message) => {
                match self.member_profile_card.update(message)? {
                    ProfileCardAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                }
            }
            SidebarMessage::PinnedDiffs(diffs) => {
                let room_id = self.room.room_id().to_owned();

                tracing::debug!(
                    "Sidebar for room {} applying {} diff(s), {} messages before",
                    room_id,
                    diffs.len(),
                    self.content.len()
                );

                let state = self.state.clone();

                for diff in diffs.into_iter().map(|d| {
                    d.map(|m| {
                        (
                            m.unique_id().0.clone(),
                            Arc::new(m.convert(&state, room_id.clone())),
                        )
                    })
                }) {
                    match diff {
                        VectorDiff::Append { values } => {
                            for (key, value) in values {
                                self.content.push(key, value);
                            }
                        }
                        VectorDiff::Clear => self.content = list::Content::new(),
                        VectorDiff::Insert {
                            index,
                            value: (key, value),
                        } => {
                            self.content.insert(index, key, value);
                        }
                        VectorDiff::PopBack => {
                            if !self.content.is_empty() {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                        VectorDiff::PopFront => {
                            if !self.content.is_empty() {
                                self.content.remove(0);
                            }
                        }
                        VectorDiff::PushBack {
                            value: (key, value),
                        } => {
                            self.content.push(key, value);
                        }
                        VectorDiff::PushFront {
                            value: (key, value),
                        } => {
                            self.content.insert(0, key, value);
                        }
                        VectorDiff::Remove { index } => {
                            self.content.remove(index);
                        }
                        VectorDiff::Reset { values } => {
                            self.content = list::Content::with_items(values.into_iter().collect());
                        }
                        VectorDiff::Set {
                            index,
                            value: (_, value),
                        } => {
                            if let Some(slot) = self.content.get_index_mut(index) {
                                *slot = value;
                            }
                        }
                        VectorDiff::Truncate { length } => {
                            while self.content.len() > length {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                    }
                }

                self.pinned_messages_version += 1;
                None
            }
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, SidebarMessage> {
        let Some(state) = self.currently_visible else {
            return w::space().into();
        };

        let content = match state {
            SidebarState::Member => w::lazy(self.member_profile_card.clone(), move |p| {
                p.view(theme, structure).map(SidebarMessage::ProfileCard)
            }),
            SidebarState::MemberList => w::lazy(self.member_profile_card.clone(), move |p| {
                p.view(theme, structure).map(SidebarMessage::ProfileCard)
            }),
            _ => return w::container("test").into(),
        };

        floating_tile(theme, structure, content)
            .height(Fill)
            .width(state.width(&structure))
            .into()
    }
}
