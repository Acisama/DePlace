use deplace_core::{
    helpers::RoomPlaceholderExt, keybinds::Keybinds, matrix_api::messages::RoomSendingExt,
};
use iced::{
    Length,
    border::Radius,
    widget::{Id, operation, text_editor},
};
use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::{
    common::*,
    components::{home::ChatHelpKey, phosphor_icon},
};

use super::timeline::messages::MessageEvent;

#[derive(Debug, Clone)]
pub enum InputMessage {
    None,
    TextAction(text_editor::Action),
    SendMessage,
    Cancel,
    RemoveReplying,
    UploadPressed,
    HelpOver(Option<HelpKey>),
}

pub enum InputAction {
    SendMessage(Task<()>),
    RemoveReplying,
    HelpOver(Option<HelpKey>),
}

#[iced_cache(Clone)]
pub struct ChatInput {
    pub timeline: Option<Arc<Timeline>>,

    #[hash]
    room_id: OwnedRoomId,

    room_watchers: RoomWatchers,

    id: Id,

    /// The `[crate::components::IcedWidget]` crate requires 'static lifetimes for caching, but `text_editor` requires a reference to the content since it stores the data internally.
    content: *mut text_editor::Content,
    #[hash]
    replying_to: Option<(Arc<MessageEvent>, OwnedEventId)>,

    keybinds: Receiver<Keybinds>,
}

impl ChatInput {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        let room_id = room.room_id().to_owned();
        Self {
            timeline: None,

            room_watchers: state.room_watchers(hashing::hash_room_default(room_id.clone())),
            room_id,

            id: Id::unique(),
            content: Box::leak(Box::new(text_editor::Content::new())),
            replying_to: None,

            keybinds: state.keybinds(),
        }
    }

    pub fn set_replies_to(&mut self, message: Arc<MessageEvent>, event_id: OwnedEventId) {
        self.replying_to = Some((message, event_id));
    }

    fn content(&self) -> &'static text_editor::Content {
        // The pointer is never changed/freed, so this is safe
        unsafe { &*self.content }
    }

    pub fn get_raw_content_pointer(&self) -> *mut text_editor::Content {
        self.content
    }

    pub fn focus(&self) -> Task<()> {
        operation::focus(self.id.clone())
    }
}

impl IcedWidget<InputMessage, InputAction> for ChatInput {
    fn update(&mut self, message: InputMessage) -> Option<InputAction> {
        match message {
            InputMessage::HelpOver(help) => Some(InputAction::HelpOver(help)),
            InputMessage::None => None,
            InputMessage::UploadPressed => None,
            InputMessage::TextAction(action) => {
                // The rendered view is dropped before this is called, so there is only one (mutable) reference to `content` at a time
                unsafe { &mut *self.content }.perform(action);
                None
            }
            InputMessage::RemoveReplying => {
                self.replying_to = None;
                Some(InputAction::RemoveReplying)
            }
            InputMessage::Cancel => {
                self.replying_to = None;
                None
            }
            InputMessage::SendMessage => {
                let Some(timeline) = self.timeline.clone() else {
                    tracing::warn!(
                        "Tried to send message in room {} without a timeline",
                        self.room_id
                    );
                    return None;
                };

                let text = self.content().text();
                let replying_to = if let Some((_, id)) = &self.replying_to {
                    Some(id.clone())
                } else {
                    None
                };

                let content = unsafe { &mut *self.content };
                content.perform(text_editor::Action::SelectAll);
                content.perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                self.replying_to = None;

                Some(InputAction::SendMessage(Task::future(async move {
                    if let Err(e) = timeline.send_message(text, replying_to).await {
                        tracing::warn!("Failed to send message: {}", e);
                    }
                })))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, InputMessage> {
        let Some(room) = self.room_watchers.get_room(&self.room_id) else {
            return w::Space::new().into();
        };

        let keybinds = self.keybinds.clone();
        let line_height = structure.font_size * 1.2;

        let button_size = line_height + structure.small_gap * 2.0;

        let reply_bar_size = structure.chat.text_size * 1.2 + structure.small_gap * 2.0;

        let help_view = create_help_view(help_state, theme, InputMessage::HelpOver);

        let input_button = |svg: &'static str, key, help| {
            w::container(
                help_view
                    .call(
                        key,
                        help,
                        w::button(phosphor_icon(svg, line_height))
                            .style(move |_, status| ButtonStyle {
                                text_color: if status.active() {
                                    theme.text.normal.into()
                                } else {
                                    theme.text.dim.into()
                                },
                                background: status.active().then_some(theme.solid_hover_bg.into()),
                                border: border::rounded(structure.semi_border_radius()),
                                ..Default::default()
                            })
                            .on_press(InputMessage::UploadPressed)
                            .padding(structure.small_gap),
                    )
                    .radius(structure.semi_border_radius()),
            )
            .padding(structure.small_gap)
        };

        let max_lines = 10.0;

        let replies_to = if let Some((msg, _)) = &self.replying_to {
            Some(msg.clone())
        } else {
            None
        };
        let is_replying_to = self.replying_to.is_some();

        let replying_banner: Element<'static, InputMessage> = match replies_to {
            Some(msg) => help_view
                .call(
                    HelpKey::Chat(crate::components::home::ChatHelpKey::Input),
                    "The message being replied to",
                    w::container(w::column![
                        w::row![
                            w::container(
                                w::text("Replying to")
                                    .size(structure.chat.text_size)
                                    .color(theme.text.normal)
                            )
                            .padding(structure.small_gap)
                            .width(Fill),
                            w::container(
                                w::button(phosphor_icon(
                                    phosphor_svgs::icon::x::BOLD,
                                    structure.chat.text_size
                                ))
                                .padding(structure.small_gap * 0.66)
                                .style(move |_, status| ButtonStyle {
                                    background: if status.active() {
                                        Some(theme.solid_hover_bg.into())
                                    } else {
                                        None
                                    },
                                    text_color: if status.active() {
                                        theme.text.normal.into()
                                    } else {
                                        theme.text.dim.into()
                                    },
                                    border: border::rounded(structure.semi_border_radius()),
                                    ..Default::default()
                                })
                                .on_press(InputMessage::RemoveReplying)
                            )
                            .center(reply_bar_size)
                        ]
                        .width(Fill),
                        w::container(Space::new())
                            .style(move |_| ContainerStyle {
                                background: Some(theme.border.into()),
                                ..Default::default()
                            })
                            .height(structure.border_thickness)
                            .width(Fill),
                        w::container(
                            msg.view(self.room_id.clone(), theme, structure, true, help_state, 0)
                                .map(|_| InputMessage::None)
                        )
                    ])
                    .style(move |_| ContainerStyle {
                        background: Some(theme.solid_bg.into()),
                        border: Border {
                            color: theme.border.into(),
                            width: structure.border_thickness,
                            radius: Radius::from(structure.inner_border_radius).bottom(0.0),
                        },
                        ..Default::default()
                    }),
                )
                .into(),
            None => Space::new().into(),
        };

        help_view
            .call(
                HelpKey::Chat(ChatHelpKey::Input),
                "The chat input, you can write messages here",
                w::column![
                    replying_banner,
                    w::stack![
                        w::text_editor(self.content())
                            .id(self.id.clone())
                            .placeholder(format!("Message {}", room.get_input_placeholder()))
                            .padding(
                                Padding::new(structure.small_gap * 2.0)
                                    .left(button_size + structure.small_gap * 2.0)
                            )
                            .on_action(InputMessage::TextAction)
                            .key_binding(move |key| {
                                if !key.modifiers.shift()
                                    && matches!(
                                        key.key,
                                        iced::keyboard::Key::Named(
                                            iced::keyboard::key::Named::Enter
                                        )
                                    )
                                {
                                    Some(text_editor::Binding::Custom(InputMessage::SendMessage))
                                } else {
                                    let keybinds = keybinds.borrow();
                                    let is_reserved =
                                        keybinds.action_for_key(&key.key, &key.modifiers).is_some();
                                    drop(keybinds);

                                    if is_reserved {
                                        None
                                    } else {
                                        text_editor::Binding::from_key_press(key)
                                    }
                                }
                            })
                            .wrapping(text::Wrapping::WordOrGlyph)
                            .line_height(text::LineHeight::Relative(1.0))
                            .height(Length::Fit.min(line_height).max(line_height * max_lines))
                            .style(move |_, status| w::text_editor::Style {
                                background: theme.solid_bg.into(),
                                border: Border {
                                    color: if status.active() {
                                        theme.accent.into()
                                    } else {
                                        theme.border.into()
                                    },
                                    width: 1.0,
                                    radius: Radius::from(structure.inner_border_radius).top(
                                        if !is_replying_to {
                                            structure.inner_border_radius
                                        } else {
                                            0.0
                                        }
                                    ),
                                },
                                placeholder: theme.text.muted.into(),
                                selection: theme.accent.into(),
                                value: theme.text.normal.into()
                            }),
                        input_button(
                            phosphor_svgs::icon::plus::BOLD,
                            HelpKey::Chat(ChatHelpKey::UploadButton),
                            "Upload button, press to upload files"
                        )
                    ]
                ]
                .width(Fill),
            )
            .radius(structure.inner_border_radius)
            .into()
    }
}
