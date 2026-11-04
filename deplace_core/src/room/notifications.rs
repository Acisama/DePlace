use std::iter::Sum;
use std::ops::AddAssign;

use matrix_sdk::sync::UnreadNotificationsCount;

#[derive(Debug, Clone, Copy, PartialEq, Default, Hash)]
pub struct NotificationCounts {
    pub highlight_count: u64,
    pub notification_count: u64,
}

impl NotificationCounts {
    pub fn total(&self) -> u64 {
        self.highlight_count.max(self.notification_count)
    }

    pub fn highlights(&self) -> Option<u64> {
        if self.highlight_count > 0 {
            Some(self.highlight_count)
        } else {
            None
        }
    }

    pub fn has_notifications(&self) -> bool {
        self.highlight_count > 0 || self.notification_count > 0
    }
}

impl From<UnreadNotificationsCount> for NotificationCounts {
    fn from(value: UnreadNotificationsCount) -> Self {
        Self {
            highlight_count: value.highlight_count,
            notification_count: value.notification_count,
        }
    }
}

impl AddAssign for NotificationCounts {
    fn add_assign(&mut self, other: Self) {
        self.highlight_count += other.highlight_count;
        self.notification_count += other.notification_count;
    }
}

impl Sum for NotificationCounts {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |acc, counts| Self {
            highlight_count: acc.highlight_count + counts.highlight_count,
            notification_count: acc.notification_count + counts.notification_count,
        })
    }
}
