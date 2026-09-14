use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use deplace_core::{ProfileLike, state::AppState};
use iced::{
    Element, Length, Task,
    keyboard::{Event, Key, key::Named},
    widget::{button, column, container, row, scrollable, space, text, text_input},
};
use macros::iced_cache;
use matrix_sdk::ruma::OwnedRoomId;
use nucleo::{
    Config, Nucleo,
    pattern::{CaseMatching, Normalization},
};

use crate::common::{IcedWidget, Structure, Theme};

pub const QUICK_SELECT_INPUT_ID: &str = "quick_select_input";

#[derive(Clone)]
#[iced_cache]
pub struct QuickSelect {
    state: AppState,

    #[hash]
    input: String,

    #[hash]
    selected_index: usize,

    matcher: Arc<Mutex<Nucleo<OwnedRoomId>>>,
}

#[derive(Clone, Debug)]
pub enum QuickSelectMessage {
    Input(String),
    SelectRoom(OwnedRoomId),
    KeyboardEvent(Event),
    RoomSwitchDone,
}

#[derive(Debug)]
pub enum QuickSelectAction {
    ChangeRoom(Task<()>),
    Close,
}

impl QuickSelect {
    pub fn new(state: &AppState) -> Self {
        let matcher = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), None, 1);
        let injector = matcher.injector();

        let mut added_rooms = HashSet::new();
        let mut add_room = |room: matrix_sdk::room::Room| {
            let room_id = room.room_id().to_owned();
            if added_rooms.insert(room_id.clone()) {
                let name = room.get_name();
                injector.push(room_id, |_, dst| {
                    dst[0] = name.into();
                });
            }
        };

        let client = state.client();
        for room in client.rooms() {
            add_room(room);
        }
        for room in state.dm_rooms().borrow().values() {
            add_room(room.clone());
        }
        for room in state.server_rooms().borrow().values() {
            add_room(room.clone());
        }
        for room in state.single_rooms().borrow().values() {
            add_room(room.clone());
        }

        Self {
            state: state.clone(),
            input: String::new(),
            matcher: Arc::new(Mutex::new(matcher)),
            selected_index: 0,
        }
    }

    fn get_displayed_rooms(&self) -> Vec<OwnedRoomId> {
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
}

impl IcedWidget<QuickSelectMessage, QuickSelectAction> for QuickSelect {
    fn update(&mut self, message: QuickSelectMessage) -> Option<QuickSelectAction> {
        match message {
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
                let room = self.state.client().get_room(&room_id);
                let state = self.state.clone();
                tracing::trace!("Quick selected room {room_id}");
                return Some(QuickSelectAction::ChangeRoom(Task::future(async move {
                    state.set_active_room(room.clone()).await;
                })));
            }
            QuickSelectMessage::RoomSwitchDone => Some(QuickSelectAction::Close),
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, QuickSelectMessage> {
        let displayed_rooms = self.get_displayed_rooms();
        let client = self.state.client();

        let mut input_field = text_input("Search for rooms...", self.input.clone())
            .id(QUICK_SELECT_INPUT_ID)
            .on_input(QuickSelectMessage::Input)
            .style(move |_iced_theme, status| match status {
                text_input::Status::Focused { .. } => text_input::Style {
                    background: theme.input.focus_background.into(),
                    border: iced::Border {
                        radius: 6.0.into(),
                        width: 1.0,
                        color: theme.input.focused_border,
                        ..Default::default()
                    },
                    placeholder: theme.text.muted,
                    value: theme.text.normal,
                    selection: theme.accent,
                },
                _ => text_input::Style {
                    background: theme.input.background.into(),
                    border: iced::Border {
                        radius: 6.0.into(),
                        width: 1.0,
                        color: theme.border,
                        ..Default::default()
                    },
                    placeholder: theme.text.muted,
                    value: theme.text.normal,
                    selection: theme.accent,
                },
            });

        if displayed_rooms.len() > 0 {
            input_field = input_field.on_submit(QuickSelectMessage::SelectRoom(
                displayed_rooms[self.selected_index].clone(),
            ));
        }

        let mut room_list = column![].spacing(4).width(Length::Fill);

        for (index, room_id) in displayed_rooms.into_iter().enumerate() {
            if let Some(room) = client.get_room(&room_id) {
                let name = room.get_name();
                let is_selected = index == self.selected_index;

                let item_button = button(text(name).color(theme.text.normal))
                    .width(Length::Fill)
                    .padding(8)
                    .on_press(QuickSelectMessage::SelectRoom(room_id))
                    .style(move |_iced_theme, status| {
                        if is_selected {
                            button::Style {
                                background: Some(theme.solid_hover_bg.into()),
                                text_color: theme.text.normal,
                                border: iced::Border {
                                    radius: 6.0.into(),
                                    width: 1.0,
                                    color: theme.accent,
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        } else if matches!(
                            status,
                            button::Status::Hovered | button::Status::Pressed
                        ) {
                            button::Style {
                                background: Some(theme.solid_hover_bg.into()),
                                text_color: theme.text.normal,
                                border: iced::Border {
                                    radius: 6.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        } else {
                            button::Style {
                                background: Some(iced::Color::TRANSPARENT.into()),
                                text_color: theme.text.normal,
                                border: iced::Border {
                                    radius: 6.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }
                    });

                room_list = room_list.push(item_button);
            }
        }

        let scrollable_list = scrollable(room_list)
            .width(Length::Fill)
            .height(Length::Fill);

        let inner_content = column![input_field, scrollable_list]
            .spacing(structure.gap)
            .width(Length::Fill)
            .height(Length::Fill);

        let card = container(inner_content)
            .padding(structure.gap)
            .width(Length::FillPortion(1))
            .height(Length::FillPortion(1))
            .style(move |_iced_theme| container::Style {
                background: Some(theme.background.into()),
                border: iced::Border {
                    radius: 10.0.into(),
                    width: 1.0,
                    color: theme.border,
                    ..Default::default()
                },
                text_color: Some(theme.text.normal),
                ..Default::default()
            });

        // Center in a 3x3 layout taking up 1/3 max width/height
        column![
            space().height(Length::FillPortion(1)),
            row![
                space().width(Length::FillPortion(1)),
                card,
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
