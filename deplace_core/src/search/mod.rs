use std::time::SystemTime;

use futures::{Stream, StreamExt};
use matrix_sdk_ui::timeline::TimelineItemContent;
use ruma::{OwnedRoomId, OwnedUserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::DePlaceRoom;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct SearchParameters {
    pub search_id: Uuid,
    pub room_ids: Vec<OwnedRoomId>,
    pub text: String,
    pub senders: Vec<OwnedUserId>,
    pub after: Option<SystemTime>,
    pub before: Option<SystemTime>,
    pub has_link: bool,
}

impl SearchParameters {
    /// Builds a tantivy query string for the search index. The indexed fields
    /// are `body` (the default search field), `sender` and `date`.
    pub fn build_query(&self) -> String {
        let mut clauses = Vec::new();

        // Quote each word so tantivy treats it as a literal term instead of
        // query syntax. Matching is case-insensitive because the `body` field
        // is tokenized with the default (lowercasing) tokenizer.
        let words: Vec<_> = self
            .text
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();

        if !words.is_empty() {
            clauses.push(format!("({})", words.join(" OR ")));
        }

        if !self.senders.is_empty() {
            let senders_clause = self
                .senders
                .iter()
                .map(|s| format!("sender:\"{}\"", s))
                .collect::<Vec<_>>()
                .join(" OR ");
            clauses.push(format!("({})", senders_clause));
        }

        // if let Some(after) = self.after {
        //     clauses.push(format!(
        //         "date:[{} TO *]",
        //         after.
        //     ));
        // }
        // if let Some(before) = self.before {
        //     clauses.push(format!(
        //         "date:[* TO {}}}",
        //         before.to_rfc3339_opts(SecondsFormat::Secs, true)
        //     ));
        // }

        if self.has_link {
            clauses.push("(http OR https)".to_string());
        }

        clauses.join(" AND ")
    }

    pub fn is_empty(&self, current_room_id: Option<OwnedRoomId>) -> bool {
        self.room_ids.is_empty()
            || (self.room_ids.first().cloned() == current_room_id)
                && self.text.is_empty()
                && self.senders.is_empty()
                && self.after.is_none()
                && self.before.is_none()
                && !self.has_link
    }
}

pub struct SearchResultUpdate {
    pub id: Uuid,
    pub room_id: OwnedRoomId,
    pub messages: Vec<TimelineItemContent>,
}

pub trait SearchExt {
    fn search(
        &self,
        search_id: Uuid,
        search_parameters: SearchParameters,
    ) -> impl Stream<Item = SearchResultUpdate>;
}

impl SearchExt for DePlaceRoom {
    fn search(
        &self,
        search_id: Uuid,
        search_parameters: SearchParameters,
    ) -> impl Stream<Item = SearchResultUpdate> {
        let room_id = self.room_id().to_owned();
        let room = self.sdk_room().clone();

        room.search_messages_events(search_parameters.build_query())
            .then(move |res| {
                let room_id = room_id.clone();
                let room = room.clone();

                async move {
                    let Ok(events) = res else {
                        return SearchResultUpdate {
                            id: search_id,
                            room_id,
                            messages: vec![],
                        };
                    };

                    let mut messages = Vec::new();

                    for msg in events {
                        if let Some(msg) = TimelineItemContent::from_event(&room, msg).await {
                            messages.push(msg);
                        }
                    }

                    SearchResultUpdate {
                        id: search_id,
                        room_id,
                        messages,
                    }
                }
            })
    }
}
