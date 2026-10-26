use std::{
    collections::{BTreeSet, HashSet},
    sync::{Arc, Mutex},
};

use crate::common::*;

use deplace_core::{
    ProfileLike,
    settings::NameDecoration,
    state::{AppState, PresenceMap},
};
use iced::{
    Alignment, Element, Length,
    keyboard::{Event, Key, key::Named},
    padding,
    widget::{column, opaque, row, space, text_input},
};
use macros::iced_cache;
use nucleo::{
    Config, Injector, Nucleo,
    pattern::{CaseMatching, Normalization},
};

use crate::common::{IcedWidget, ProfileRenderExt, Structure, Theme, themed_scrollable};

pub const QUICK_SELECT_INPUT_ID: &str = "quick_select_input";

#[iced_cache(Clone)]
pub struct QuickSelect {
    state: AppState,
    avatar_cache: AvatarCache,

    room_watchers: RoomWatchers,

    name_decoration: Receiver<NameDecoration>,

    #[hash]
    input: String,

    #[hash]
    selected_index: usize,
    #[hash]
    hovered_index: Option<usize>,

    matcher: Arc<Mutex<Nucleo<OwnedRoomId>>>,
    injector: Injector<OwnedRoomId>,
    pooled_rooms: Arc<Mutex<HashSet<OwnedRoomId>>>,

    presence_map: Receiver<PresenceMap>,
}

impl ExtraHash for QuickSelect {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name_decoration.borrow().hash(state);
    }
}

#[derive(Clone, Debug)]
pub enum QuickSelectMessage {
    Input(String),
    SelectRoom(OwnedRoomId),
    KeyboardEvent(Event),
    RoomSwitchDone,
    NeedsAvatar(OwnedMxcUri),
    HoverRoom(usize),
    UnhoverRoom(usize),
}

impl NeedsAvatarExt for QuickSelectMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatar(uri)
    }
}

#[derive(Debug)]
pub enum QuickSelectAction {
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<DePlaceRoom>),
    Close,
}

impl QuickSelect {
    pub fn new(state: &AppState) -> Self {
        let matcher = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), None, 1);
        let injector = matcher.injector();

        let room_watchers = state.room_watchers(hashing::hash_all_rooms_default());

        Self {
            avatar_cache: state.avatar_cache().clone(),

            avatar_states_for_hash: BTreeSet::new(),

            name_decoration: state.settings().name_decoration.watch(),

            room_watchers,

            state: state.clone(),
            input: String::new(),
            matcher: Arc::new(Mutex::new(matcher)),
            injector,
            pooled_rooms: Arc::new(Mutex::new(HashSet::new())),

            presence_map: state.presence_map(),

            selected_index: 0,
            hovered_index: None,
        }
    }

    // Keeps the matcher pool in sync with room_watchers instead of a fixed snapshot from new().
    fn sync_room_pool(&self) {
        let Ok(mut pooled_rooms) = self.pooled_rooms.lock() else {
            return;
        };

        for room in self.room_watchers.all_rooms().get_all_rooms() {
            if pooled_rooms.contains(room.room_id()) {
                continue;
            }
            let room_id = room.room_id().to_owned();
            pooled_rooms.insert(room_id.clone());
            let name = room.get_name();
            self.injector.push(room_id, move |_, dst| {
                dst[0] = name.clone().into();
            });
        }
    }

    fn get_displayed_rooms(&self) -> Vec<OwnedRoomId> {
        self.sync_room_pool();

        if self.input.trim().is_empty() {
            let client = self.state.client();
            self.state
                .breadcrumbs()
                .recent_rooms()
                .iter()
                .skip(1)
                .filter(|id| client.get_room(id).is_some())
                .cloned()
                .collect()
        } else if let Ok(mut matcher) = self.matcher.lock() {
            matcher.tick(10);
            let snapshot = matcher.snapshot();
            let matched_count = snapshot.matched_item_count();
            snapshot
                .matched_items(0..matched_count)
                .map(|item| item.data.clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    fn clear(&mut self) {
        self.input = String::from("");
        self.selected_index = 0;
    }
}

impl IcedWidget<QuickSelectMessage, QuickSelectAction> for QuickSelect {
    fn update(&mut self, message: QuickSelectMessage) -> Option<QuickSelectAction> {
        match message {
            QuickSelectMessage::HoverRoom(index) => {
                self.hovered_index = Some(index);
                None
            }
            QuickSelectMessage::UnhoverRoom(index) => {
                if self.hovered_index == Some(index) {
                    self.hovered_index = None;
                }
                None
            }
            QuickSelectMessage::Input(input) => {
                if input != self.input {
                    self.input = input.clone();
                    self.selected_index = 0; // Always focus first item on new search query
                    if let Ok(mut matcher) = self.matcher.lock() {
                        matcher.pattern.reparse(
                            0,
                            &self.input,
                            CaseMatching::Ignore,
                            Normalization::Smart,
                            false,
                        );
                    }
                }
                None
            }
            QuickSelectMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(QuickSelectAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
            // QuickSelectMessage::SelectRoom(room_id) => Some(QuickSelectAction::Run(Some(room_id))),
            QuickSelectMessage::KeyboardEvent(event) => {
                if let Event::KeyPressed { key, .. } = event {
                    let displayed_rooms = self.get_displayed_rooms();
                    let count = displayed_rooms.len();

                    if count > 0 {
                        // Keep selected_index in bounds if items changed
                        if self.selected_index >= count {
                            self.selected_index = 0;
                        }

                        match key {
                            Key::Named(Named::ArrowDown) => {
                                self.selected_index = (self.selected_index + 1) % count;
                            }
                            Key::Named(Named::ArrowUp) => {
                                if self.selected_index == 0 {
                                    self.selected_index = count - 1;
                                } else {
                                    self.selected_index -= 1;
                                }
                            }
                            Key::Named(Named::Escape) => {
                                self.clear();
                                return Some(QuickSelectAction::Close);
                            }
                            // Key::Named(Named::Enter) => {
                            //     if let Some(room_id) = displayed_rooms.get(self.selected_index) {
                            //         let room = self.state.client().get_room(room_id);
                            //         let state = self.state.clone();
                            //         return Some(QuickSelectAction::Run(Task::future(
                            //             async move {
                            //                 state.set_active_room(room).await;
                            //             },
                            //         )));
                            //     }
                            // }
                            _ => {}
                        }
                    }
                }
                None
            }
            QuickSelectMessage::SelectRoom(room_id) => {
                let room = self.room_watchers.get_room(&room_id);
                self.clear();
                tracing::trace!("Quick selected room {room_id}");
                Some(QuickSelectAction::ChangeRoom(room.clone()))
            }
            QuickSelectMessage::RoomSwitchDone => Some(QuickSelectAction::Close),
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> Element<'static, QuickSelectMessage> {
        let displayed_rooms = self.get_displayed_rooms();

        let mut input_field = text_input("Search for rooms...", self.input.clone())
            .id(QUICK_SELECT_INPUT_ID)
            .on_input(QuickSelectMessage::Input)
            .style(move |_iced_theme, status| match status {
                text_input::Status::Focused { .. } => text_input::Style {
                    background: theme.input.focus_background.into(),
                    border: iced::Border {
                        radius: 6.0.into(),
                        width: 1.0,
                        color: theme.input.focused_border.into(),
                    },
                    placeholder: theme.text.muted.into(),
                    value: theme.text.normal.into(),
                    selection: theme.accent.into(),
                },
                _ => text_input::Style {
                    background: theme.input.background.into(),
                    border: iced::Border {
                        radius: 6.0.into(),
                        width: 1.0,
                        color: theme.border.into(),
                    },
                    placeholder: theme.text.muted.into(),
                    value: theme.text.normal.into(),
                    selection: theme.accent.into(),
                },
            });

        if !displayed_rooms.is_empty() {
            input_field = input_field.on_submit(QuickSelectMessage::SelectRoom(
                displayed_rooms[self.selected_index].clone(),
            ));
        }

        let mut room_list = column![]
            .spacing(structure.small_gap)
            .padding(padding::right(structure.gap))
            .width(Length::Fill);

        let name_decoration = *self.name_decoration.borrow();
        let presence_map = &self.presence_map.borrow();

        let avatar_cache = &self.avatar_cache;
        for (index, room_id) in displayed_rooms.into_iter().enumerate() {
            if let Some(room) = self.room_watchers.get_room(&room_id) {
                let is_selected = index == self.selected_index;
                let is_hovered = self.hovered_index == Some(index);

                let icon_size = structure.font_size * 1.2;

                let text_color = if is_hovered {
                    theme.text.normal.into()
                } else {
                    theme.text.dim.into()
                };

                let background_color = if is_selected {
                    theme.solid_hover_bg.into()
                } else {
                    theme.solid_bg.into()
                };

                let item_button = w::mouse_area(
                    w::container(
                        w::row![
                            context_room_icon(
                                &room,
                                icon_size,
                                theme,
                                structure,
                                presence_map,
                                avatar_cache,
                                background_color
                            ),
                            room.render_name_decorated(
                                structure.font_size,
                                name_decoration,
                                text_color
                            )
                        ]
                        .width(Length::Fill)
                        .align_y(Alignment::Center)
                        .spacing(structure.small_gap),
                    )
                    .padding(structure.small_gap)
                    .style(move |_| ContainerStyle {
                        background: is_selected.then_some(theme.solid_hover_bg.into()),
                        text_color: Some(text_color),
                        border: Border {
                            color: theme.border.into(),
                            width: if is_hovered || is_selected {
                                structure.border_thickness
                            } else {
                                0.0
                            },
                            radius: structure.inner_border_radius.into(),
                        },
                        ..Default::default()
                    }),
                )
                .on_press(QuickSelectMessage::SelectRoom(room_id));

                room_list = room_list.push(item_button);
            }
        }

        let scrollable_list = themed_scrollable(room_list, theme, structure)
            .width(Length::Fill)
            .height(Length::Fill);

        let inner_content = opaque(
            floating_tile(
                theme,
                structure,
                column![input_field, scrollable_list]
                    .spacing(structure.gap)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .padding(structure.gap)
            .width(Length::FillPortion(1))
            .height(Length::FillPortion(1)),
        );

        // Center in a 3x3 layout taking up 1/3 max width/height
        column![
            space().height(Length::FillPortion(1)),
            row![
                space().width(Length::FillPortion(1)),
                inner_content,
                space().width(Length::FillPortion(1)),
            ]
            .width(Length::Fill)
            .height(Length::FillPortion(1)),
            space().height(Length::FillPortion(1)),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
