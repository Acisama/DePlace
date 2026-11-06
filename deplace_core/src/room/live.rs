use matrix_sdk::Room;

use super::notifications::NotificationCounts;

#[derive(Clone, Debug, PartialEq)]
pub struct RoomLive {
    pub(crate) notification_counts: NotificationCounts,
}

impl RoomLive {
    pub fn new(room: &Room) -> Self {
        Self {
            notification_counts: room.unread_notification_counts().into(),
        }
    }

    pub fn notification_counts(&self) -> NotificationCounts {
        self.notification_counts
    }
}
