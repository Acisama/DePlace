use crate::common::*;

use super::ChatMessage;

pub fn render_room_chat_invited(
    room: DePlaceRoom,
    theme: Theme,
    structure: Structure,
) -> Element<'static, ChatMessage> {
    let text = format!(
        "Do you want to {}",
        if let Some(member) = room.other_dm_member() {
            format!("start a chat with {}", member.displayname())
        } else {
            format!("join {}", room.displayname())
        }
    );

    w::column![w::text(text), room.render_name(structure.font_size)].boxed()
}
