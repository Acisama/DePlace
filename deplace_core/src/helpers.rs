use anyhow::Result;
use chrono::{DateTime, Local};
use matrix_sdk::{
    Client, Media, Room,
    media::{MediaFormat, MediaRequestParameters},
};
use ruma::events::room::MediaSource;

use crate::{NameExt, get_other_member, state::MembershipMap};

pub fn format_message_long_date(date: DateTime<Local>) -> String {
    let hour_str = "%H:%M";
    let date_str = "%d/%m/%Y";
    let now = Local::now();

    match (date.date_naive() - now.date_naive()).num_days() {
        0 => date.format(&format!("Today, {}", hour_str)).to_string(),
        -1 => date.format(&format!("Yesterday, {}", hour_str)).to_string(),
        -6..-1 => date
            .format(&format!("%a {}, {}", date_str, hour_str))
            .to_string(),
        _ => date
            .format(&format!("{}, {}", date_str, hour_str))
            .to_string(),
    }
}

pub fn format_date_divider(date: DateTime<Local>) -> String {
    let now = Local::now();

    let is_today = date.date_naive() == now.date_naive();
    let is_yesterday = date.date_naive() == (now - chrono::Duration::days(1)).date_naive();
    let is_week_ago = date > now - chrono::Duration::days(7);

    if is_today {
        "Today".to_string()
    } else if is_yesterday {
        "Yesterday".to_string()
    } else {
        date.format(&format!("{}%d %B %Y", if is_week_ago { "%a " } else { "" }))
            .to_string()
    }
}

pub fn format_message_short_date(date: DateTime<Local>) -> String {
    date.format("%H:%M").to_string()
}

pub trait RoomPlaceholderExt {
    fn get_input_placeholder(&self, map: &MembershipMap) -> String;
}

impl RoomPlaceholderExt for Room {
    fn get_input_placeholder(&self, map: &MembershipMap) -> String {
        if self.is_dm()
            && let Some(member) = get_other_member(self.own_user_id(), map, self.room_id())
        {
            format!("Message @{}", member.get_name())
        } else {
            format!("Message #{}", self.get_name())
        }
    }
}

impl RoomPlaceholderExt for Option<Room> {
    fn get_input_placeholder(&self, map: &MembershipMap) -> String {
        if let Some(room) = self {
            room.get_input_placeholder(map)
        } else {
            "Type a message...".to_string()
        }
    }
}

pub trait MatrixClientExt {
    fn get_file_content(
        &self,
        source: MediaSource,
        use_cache: bool,
    ) -> impl std::future::Future<Output = matrix_sdk::Result<Vec<u8>>> + Send;
}

impl MatrixClientExt for Media {
    /// Convenience method to get the content of a file media source.
    async fn get_file_content(
        &self,
        source: MediaSource,
        use_cache: bool,
    ) -> matrix_sdk::Result<Vec<u8>> {
        self.get_media_content(
            &MediaRequestParameters {
                source,
                format: MediaFormat::File,
            },
            use_cache,
        )
        .await
    }
}
