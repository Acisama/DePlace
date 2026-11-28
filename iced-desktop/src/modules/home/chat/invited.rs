use iced::Alignment;

use crate::common::*;

use super::ChatMessage;

pub fn render_room_chat_invited(
    room: DePlaceRoom,
    invited_by: Option<Member>,
    theme: Theme,
    structure: Structure,
) -> Element<'static, ChatMessage> {
    let text = format!(
        "Do you want to {} {}",
        if room.is_dm() {
            "start a chat with"
        } else {
            "join"
        },
        room.get_input_placeholder()
    );

    w::container(
        w::column![
            w::text(text)
                .size(structure.large_font_size)
                .color(theme.text.normal),
            w::container(room.render_icon(structure.chat.icon_size * 2.0)).padding(structure.gap),
            room.render_name(structure.font_size)
        ]
        .align_x(Alignment::Center),
    )
    .width(Fill)
    .height(Fill)
    .center(Fill)
    .boxed()
}
