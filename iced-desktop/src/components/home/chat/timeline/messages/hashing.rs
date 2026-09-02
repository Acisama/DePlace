use crate::common::*;

use super::{MessageEvent, TimelineItem, TimelineItemKind};

impl Hash for TimelineItem {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Other things are static, so hashing doesn't need to include them
        if let TimelineItemKind::Message(msg) = &self.kind {
            msg.hash(state);
        }

        self.id.hash(state);
    }
}

impl Hash for MessageEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {}
}
