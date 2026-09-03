use std::fmt::Display;

use chrono::{DateTime, Local};
use matrix_sdk::{
    Media, Room,
    media::{MediaFormat, MediaRequestParameters},
    room::RoomMember,
};
use matrix_sdk_ui::timeline::MemberProfileChange;
use ruma::events::{
    RedactContent, StateEventContentChange, StaticStateEventContent,
    room::{MediaSource, member::Change},
};

use crate::{ProfileLike, state::MembershipMap};

pub trait RoomPlaceholderExt {
    fn get_input_placeholder(&self) -> String;
}

impl RoomPlaceholderExt for Room {
    fn get_input_placeholder(&self) -> String {
        if self.is_dm()
            && let Some(name) = self.cached_display_name()
        {
            format!("@{}", name)
        } else {
            format!("#{}", self.get_name())
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

pub trait DisplayString {
    fn display_string(&self) -> String;
}

impl DisplayString for MemberProfileChange {
    fn display_string(&self) -> String {
        let mut changes = Vec::new();

        if let Some(Change { old, new }) = self.displayname_change() {
            if let Some(new) = new {
                if let Some(old) = old {
                    changes.push(format!(
                        "changed their display name from '{}' to '{}'",
                        old, new
                    ));
                } else {
                    changes.push(format!("set their display name to '{}'", new));
                }
            } else {
                changes.push("removed their display name".to_string());
            }
        }

        if let Some(Change { old, new }) = &self.avatar_url_change() {
            if new.is_some() && old.is_none() {
                changes.push("set a profile picture".to_string());
            } else if new.is_none() && old.is_some() {
                changes.push("removed their profile picture".to_string());
            } else {
                changes.push("changed their profile picture".to_string());
            }
        }

        changes.join(" and ")
    }
}

pub enum EventChange<T> {
    Unset(T),
    Set(T),
    Changed { old: T, new: T },
    Something,
}

impl<T> EventChange<T> {
    pub fn display_string_with_render_fn<C: Display>(
        &self,
        render_fn: impl Fn(&T) -> C,
        property_name: &str,
    ) -> String {
        match self {
            EventChange::Unset(_) => format!("unset {}", property_name),
            EventChange::Set(_) => format!("set {}", property_name),
            EventChange::Changed { old, new } => {
                format!(
                    "changed {} from {} to {}",
                    property_name,
                    render_fn(old),
                    render_fn(new)
                )
            }
            EventChange::Something => format!("changed {}", property_name),
        }
    }
}

impl<T: Display> EventChange<T> {
    pub fn display_string(&self, property_name: &str) -> String {
        match self {
            EventChange::Unset(_) => format!("unset {}", property_name),
            EventChange::Set(_) => format!("set {}", property_name),
            EventChange::Changed { old, new } => {
                format!("changed {} from {} to {}", property_name, old, new)
            }
            EventChange::Something => format!("changed {}", property_name),
        }
    }
}

pub fn get_current_and_prev<T, C>(
    change: &StateEventContentChange<T>,
    get_val1: impl FnOnce(&T) -> Option<C>,
    get_val2: impl FnOnce(&<T as StaticStateEventContent>::PossiblyRedacted) -> Option<C>,
) -> EventChange<C>
where
    T: RedactContent + Clone + StaticStateEventContent,
{
    if let StateEventContentChange::Original {
        content,
        prev_content,
    } = change
    {
        let current = get_val1(content);
        let prev = prev_content.as_ref().and_then(get_val2);

        match (current, prev) {
            (Some(current), Some(prev)) => EventChange::Changed {
                old: prev,
                new: current,
            },
            (Some(current), None) => EventChange::Set(current),
            (None, Some(prev)) => EventChange::Unset(prev),
            (None, None) => EventChange::Something,
        }
    } else {
        EventChange::Something
    }
}

#[macro_export]
macro_rules! get_change {
    ($change:expr, |$arg:ident| $body:expr) => {
        $crate::helpers::get_current_and_prev(
            $change,
            |$arg| $body, // The compiler type-checks $arg as &T here
            |$arg| $body, // The compiler type-checks $arg as &T::PossiblyRedacted here
        )
    };
}

pub trait RoomExt {
    fn get_other_member(&self, map: &MembershipMap) -> Option<RoomMember>;
}

impl RoomExt for Room {
    fn get_other_member(&self, map: &MembershipMap) -> Option<RoomMember> {
        let own_id = self.own_user_id();
        map.get(self.room_id()).and_then(|members| {
            members
                .iter()
                .find(|(id, _)| *id != own_id)
                .map(|(_, m)| m.clone())
        })
    }
}
