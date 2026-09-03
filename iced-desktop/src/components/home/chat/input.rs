use deplace_core::{helpers::RoomPlaceholderExt, matrix_api::messages::RoomSendingExt};
use iced::{
    Length,
    widget::{Id, operation, text_editor},
};
use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::common::*;

#[derive(Debug, Clone)]
pub enum InputMessage {
    None,
    TextAction(text_editor::Action),
    SendMessage,
    Cancel,
    RemoveReplying,
    UploadPressed,
}

pub enum InputAction {
    Run(Task<()>),
    None,
}

#[iced_cache(Clone)]
pub struct ChatInput {
    pub timeline: Option<Arc<Timeline>>,

    #[hash]
    room_id: OwnedRoomId,

    placeholder: String,

    id: Id,

    /// The `[crate::components::IcedWidget]` crate requires 'static lifetimes for caching, but `text_editor` requires a reference to the content since it stores the data internally.
    content: *mut text_editor::Content,
    #[hash]
    replying_to: Option<OwnedEventId>,
}

impl ChatInput {
    pub fn new(room: &Room) -> Self {
        Self {
            timeline: None,
            room_id: room.room_id().to_owned(),

            placeholder: format!("Message {}", room.get_input_placeholder()),

            id: Id::unique(),
            content: Box::leak(Box::new(text_editor::Content::new())),
            replying_to: None,
        }
    }

    pub fn set_replies_to(&mut self, id: OwnedEventId) {
        self.replying_to = Some(id);
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
    fn update(&mut self, message: InputMessage) -> InputAction {
        match message {
            InputMessage::None => {}
            InputMessage::UploadPressed => {}
            InputMessage::TextAction(action) => {
                // The rendered view is dropped before this is called, so there is only one (mutable) reference to `content` at a time
                unsafe { &mut *self.content }.perform(action);
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

                let text = self.content().text();
                let replying_to = self.replying_to.clone();

                let content = unsafe { &mut *self.content };
                content.perform(text_editor::Action::SelectAll);
                content.perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                self.replying_to = None;

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
        let line_height = structure.font_size * 1.2;

        let button_size = line_height + structure.small_gap * 2.0;

        let input_button = |svg: &'static str| {
            w::container(
                w::button(phosphor_icon(svg, line_height))
                    .style(move |_, status| ButtonStyle {
                        text_color: if status.active() {
                            theme.text.normal
                        } else {
                            theme.text.dim
                        },
                        background: status.active().then_some(theme.solid_hover_bg.into()),
                        border: border::rounded(structure.semi_border_radius()),
                        ..Default::default()
                    })
                    .on_press(InputMessage::UploadPressed)
                    .padding(structure.small_gap),
            )
            .padding(structure.small_gap)
        };

        let max_lines = 10.0;

        w::stack![
            w::text_editor(self.content())
                .id(self.id.clone())
                .placeholder(self.placeholder.clone())
                .padding(
                    Padding::new(structure.small_gap * 2.0)
                        .left(button_size + structure.small_gap * 2.0)
                )
                .on_action(InputMessage::TextAction)
                .key_binding(|key| {
                    if !key.modifiers.shift()
                        && matches!(
                            key.key,
                            iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
                        )
                    {
                        Some(text_editor::Binding::Custom(InputMessage::SendMessage))
                    } else {
                        text_editor::Binding::from_key_press(key)
                    }
                })
                .wrapping(text::Wrapping::WordOrGlyph)
                .line_height(text::LineHeight::Relative(1.0))
                .height(Length::Fit.min(line_height).max(line_height * max_lines))
                .style(move |_, status| w::text_editor::Style {
                    background: theme.solid_bg.into(),
                    border: Border {
                        color: if status.active() {
                            theme.accent
                        } else {
                            theme.border
                        },
                        width: 1.0,
                        radius: structure.inner_border_radius.into(),
                    },
                    placeholder: theme.text.muted,
                    selection: theme.accent,
                    value: theme.text.normal
                }),
            input_button(phosphor_svgs::icon::plus::BOLD),
        ]
        .width(Fill)
        .into()
    }
}
