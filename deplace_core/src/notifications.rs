use std::sync::Arc;

use dashmap::DashMap;
use matrix_sdk::{
    Client, Room,
    event_handler::Ctx,
    media::{MediaEventContent, MediaFormat, MediaThumbnailSettings},
    room::RoomMember,
};
use notify_rust::{Notification, NotificationHandle, NotificationResponse};
use ruma::{
    EventId, OwnedEventId, OwnedRoomId,
    api::client::receipt::create_receipt::v3::ReceiptType,
    events::{
        receipt::ReceiptThread,
        room::{
            MediaSource,
            message::{MessageType, OriginalSyncRoomMessageEvent},
        },
    },
    push::{Action, Tweak},
};

use crate::{
    APP_HUMAN_NAME,
    rooms::DePlaceRoom,
    state::{AppState, ImportantPaths},
};

/// Converts a [`core::time::Duration`] to a human-readable string, using seconds, minutes, and hours.
fn duration_to_string(duration: core::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs == 0 {
        "now".to_string()
    } else if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

/// Fetches the sender's avatar as a small thumbnail and writes it to the app's cache
/// directory, returning the path to use as a notification icon. The file is keyed by
/// the avatar's media ID, so it's only downloaded and written once per distinct avatar.
async fn cached_avatar_icon_path(paths: &ImportantPaths, member: &RoomMember) -> Option<String> {
    let media_id = member.avatar_url()?.media_id().ok()?.to_string();

    let path = paths.cache_dir.join(&media_id);

    if !path.exists() {
        let settings = MediaThumbnailSettings::new(128u32.into(), 128u32.into());
        let bytes = match member.avatar(MediaFormat::Thumbnail(settings)).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!("Failed to fetch avatar for notification icon: {}", e);
                return None;
            }
        };

        if let Err(e) = tokio::fs::write(&path, bytes).await {
            tracing::warn!("Failed to write avatar icon to cache: {}", e);
            return None;
        }
    }

    Some(path.to_string_lossy().into_owned())
}

/// Fetches a small preview thumbnail for an image or video message and writes it to the
/// app's cache directory, returning the path to use as a notification preview image. The
/// file is keyed by the thumbnail's media ID, so it's only downloaded and written once per
/// distinct piece of media.
async fn cached_media_preview_path(
    paths: &ImportantPaths,
    client: &Client,
    content: &impl MediaEventContent,
) -> Option<String> {
    let source = content.thumbnail_source()?;
    let uri = match &source {
        MediaSource::Plain(uri) => uri,
        MediaSource::Encrypted(file) => &file.url,
    };
    let media_id = uri.media_id().ok()?.to_string();

    let path = paths.cache_dir.join(format!("preview-{media_id}"));

    if !path.exists() {
        let settings = MediaThumbnailSettings::new(256u32.into(), 256u32.into());
        let bytes = match client.media().get_thumbnail(content, settings, true).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!("Failed to fetch media preview for notification: {}", e);
                return None;
            }
        };

        if let Err(e) = tokio::fs::write(&path, bytes).await {
            tracing::warn!("Failed to write media preview to cache: {}", e);
            return None;
        }
    }

    Some(path.to_string_lossy().into_owned())
}

/// Function to be added as event handler to the client
///
/// Displays a notification
pub async fn on_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    actions: Vec<Action>,
    state: Ctx<AppState>,
) {
    if !actions.iter().any(|a| a.should_notify()) {
        return;
    }

    let notification_manager = state.notification_manager();

    let is_highlight = actions
        .iter()
        .any(|a| matches!(a, Action::SetTweak(Tweak::Highlight(_))));

    let focused = state.window_focused();
    let current = state
        .active_room()
        .borrow()
        .as_ref()
        .map(|r| r.room_id().to_owned());
    if focused && current.as_deref() == Some(room.room_id()) {
        return;
    }

    tracing::debug!(
        "Notification: room={} is_highlight={}",
        room.room_id(),
        is_highlight
    );

    let sender = event.sender;
    let member = match room.get_member(&sender).await {
        Ok(member) => member,
        Err(e) => {
            tracing::warn!("Failed to get member: {}", e);
            return;
        }
    };

    let name = member
        .as_ref()
        .map(|m| m.name().to_string())
        .unwrap_or(sender.to_string());

    let icon = match &member {
        Some(member) => cached_avatar_icon_path(state.important_paths(), member).await,
        None => None,
    };

    let (text, image_preview) = match event.content.msgtype {
        MessageType::Audio(audio) => {
            let duration = audio.info.and_then(|i| i.duration);

            (
                format!(
                    "{name} sent an audio message{}",
                    duration.map(duration_to_string).unwrap_or_default()
                ),
                None,
            )
        }
        MessageType::Emote(emote) => (emote.body, None),
        MessageType::File(file) => {
            let filename = file.filename();

            (format!("Sent a file: {filename}"), None)
        }
        MessageType::Image(image) => {
            let preview =
                cached_media_preview_path(state.important_paths(), &state.client(), &image).await;

            (
                image.caption().unwrap_or(image.filename()).to_string(),
                preview,
            )
        }
        MessageType::Location(_loc) => ("Sent a location".to_string(), None),
        MessageType::Notice(notice) => (format!("Sent a notice: {}", notice.body), None),
        MessageType::ServerNotice(notice) => {
            (format!("Sent a server notice: {}", notice.body), None)
        }
        MessageType::Text(text) => (text.body, None),
        MessageType::Video(video) => {
            let preview =
                cached_media_preview_path(state.important_paths(), &state.client(), &video).await;

            (
                video.caption().unwrap_or(video.filename()).to_string(),
                preview,
            )
        }
        _ => {
            return;
        }
    };

    let title = if room.compute_is_dm().await.unwrap_or(false) {
        name
    } else {
        match room.display_name().await {
            Ok(room_name) => format!("{name} in {room_name}"),
            Err(e) => {
                tracing::error!("Failed to get room name: {e}");
                name
            }
        }
    };

    let mut notification = Notification::new();

    let notification = if let Some(icon) = icon {
        notification.icon(&icon)
    } else {
        notification.auto_icon()
    }
    .summary(&title)
    .body(&text)
    .appname(APP_HUMAN_NAME)
    .action("mark_read", "Mark as Read")
    .action("dismiss", "Dismiss")
    .action("default", "default");

    let notification = if let Some(preview) = &image_preview {
        notification.image_path(preview)
    } else {
        notification
    };

    let handle = match notification.show() {
        Ok(handle) => handle,
        Err(e) => {
            tracing::warn!("Failed to send notification: {:?}", e);
            return;
        }
    };

    notification_manager.add_room_notification(
        event.event_id,
        room.room_id().to_owned(),
        handle,
        state.clone(),
        room,
    );
}

#[derive(Default, Debug, Clone)]
pub struct NotificationManager {
    room_notifications: Arc<DashMap<OwnedEventId, NotificationHandle>>,
}

impl NotificationManager {
    pub fn add_room_notification(
        &self,
        event_id: OwnedEventId,
        room_id: OwnedRoomId,
        handle: NotificationHandle,
        state: AppState,
        room: Room,
    ) {
        self.room_notifications.insert(event_id.clone(), handle);

        let notifications = self.clone();
        tokio::spawn(async move {
            let Some(entry) = notifications.get_room_notification(&event_id) else {
                return;
            };

            let mut response = None;

            entry
                .wait_for_action_async(|r| response = Some(r.clone()))
                .await;

            drop(entry);

            let Some(response) = response else {
                notifications.remove_room_notification(&event_id);
                return;
            };

            match response {
                NotificationResponse::Action(action) => match action.as_str() {
                    "mark_read" => {
                        if let Err(e) = room
                            .send_single_receipt(
                                ReceiptType::Read,
                                ReceiptThread::Unthreaded,
                                event_id.clone(),
                            )
                            .await
                        {
                            tracing::warn!("Failed to send read receipt: {}", e);
                        }
                    }
                    "dismiss" => {
                        notifications.remove_room_notification(&event_id);
                    }
                    _ => {
                        tracing::trace!("Unknown action: {}", action);
                    }
                },
                NotificationResponse::Default => {
                    tracing::trace!("Notification in room {} clicked", room_id);
                    state
                        .set_active_server(
                            state
                                .set_active_room(Some(
                                    DePlaceRoom::from_room(room, state.own_device_ref()).await,
                                ))
                                .await,
                            false,
                        )
                        .await;
                }
                // Unfortunately only supported on MacOS
                NotificationResponse::Reply(_) => {}
                NotificationResponse::Closed(_) => {}
            };

            notifications.remove_room_notification(&event_id);
        });
    }

    pub fn get_room_notification(
        &self,
        event_id: &EventId,
    ) -> Option<dashmap::mapref::one::Ref<'_, OwnedEventId, NotificationHandle>> {
        self.room_notifications.get(event_id)
    }

    pub fn remove_room_notification(&self, event_id: &EventId) {
        if let Some((_, handle)) = self.room_notifications.remove(event_id) {
            handle.close();
        }
    }

    pub fn update_room_notification(
        &self,
        event_id: &OwnedEventId,
        update: impl FnOnce(&mut NotificationHandle),
    ) {
        if let Some(mut handle) = self.room_notifications.get_mut(event_id) {
            update(&mut handle);
            if let Err(e) = handle.update() {
                tracing::error!("Failed to update notification: {}", e);
            }
        }
    }
}
