use deplace_core::{helpers::RoomPlaceholderExt, matrix_api::messages::RoomSendingExt};
use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::common::*;

#[derive(Debug, Clone)]
pub enum InputMessage {
    TextChanged(String),
    SendMessage,
    Cancel,
    RemoveReplying,
    UploadPressed,
}

pub enum InputAction {
    Run(Task<()>),
    None,
}

#[iced_cache]
pub struct ChatInput {
    pub timeline: Option<Arc<Timeline>>,
    room_id: OwnedRoomId,

    placeholder: String,

    #[hash]
    focused: bool,
    #[hash]
    text: String,
    #[hash]
    replying_to: Option<OwnedEventId>,
}

impl ChatInput {
    pub fn new(room: &Room) -> Self {
        Self {
            timeline: None,
            room_id: room.room_id().to_owned(),

            placeholder: room.get_input_placeholder(),

            focused: false,
            text: String::new(),
            replying_to: None,
        }
    }

    pub fn set_replies_to(&mut self, id: OwnedEventId) {
        self.replying_to = Some(id);
    }
}

impl IcedWidget<InputMessage, InputAction> for ChatInput {
    fn update(&mut self, message: InputMessage) -> InputAction {
        match message {
            InputMessage::UploadPressed => {}
            InputMessage::TextChanged(text) => {
                self.text = text;
            }
            InputMessage::RemoveReplying => {
                self.replying_to = None;
            }
            InputMessage::Cancel => {
                self.replying_to = None;
            }
            InputMessage::SendMessage => {
                let Some(timeline) = self.timeline.clone() else {
                    tracing::warn!(
                        "Tried to send message in room {} without a timeline",
                        self.room_id
                    );
                    return InputAction::None;
                };

                let text = self.text.clone();
                let replying_to = self.replying_to.clone();

                return InputAction::Run(Task::future(async move {
                    if let Err(e) = timeline.send_message(text, replying_to).await {
                        tracing::warn!("Failed to send message: {}", e);
                    }
                }));
            }
        }

        InputAction::None
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, InputMessage> {
        let focused = self.focused;

        let icon_size = structure.chat.input_height - structure.small_gap * 4.0;

        let input_button = |svg: &'static str| {
            w::button(phosphor_icon(svg, icon_size))
                .style(move |_, status| ButtonStyle {
                    text_color: if status.active() {
                        theme.text.normal
                    } else {
                        theme.text.dim
                    },
                    background: status.active().then_some(theme.solid_hover_bg.into()),
                    ..Default::default()
                })
                .on_press(InputMessage::UploadPressed)
                .padding(structure.small_gap)
        };

        w::container(
            w::row![
                input_button(phosphor_svgs::icon::plus::BOLD),
                w::text_input(self.placeholder.clone(), self.text.clone())
                    .on_input(InputMessage::TextChanged)
                    .on_submit(InputMessage::SendMessage)
                    .style(move |_, _| w::text_input::Style {
                        background: theme.solid_bg.into(),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 0.0.into(),
                        },
                        placeholder: theme.text.muted,
                        selection: theme.accent,
                        value: theme.text.normal
                    })
            ]
            .width(Fill)
            .height(Fill),
        )
        .width(Fill)
        .height(structure.chat.input_height)
        .style(move |_| ContainerStyle {
            text_color: Some(theme.text.normal),
            background: Some(theme.solid_bg.into()),
            border: Border {
                color: if focused { theme.accent } else { theme.border },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            ..Default::default()
        })
        .padding(structure.small_gap)
        .into()
    }
}
