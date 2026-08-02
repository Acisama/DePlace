// TODO: Remove this
#![allow(unused)]

use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose;
use livekit::e2ee::EncryptionType;
use livekit::e2ee::key_provider::{KeyProvider, KeyProviderOptions};
use livekit::track::RemoteTrack;
use matrix_sdk::deserialized_responses::ProcessedToDeviceEvent;
use matrix_sdk::event_handler::Ctx;
use matrix_sdk::ruma::api::client::rtc::RtcTransport;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
// use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

// use cpal::traits::{DeviceTrait, HostTrait};
// use futures::StreamExt;
// use livekit::track::{LocalAudioTrack, LocalTrack, RemoteAudioTrack, RemoteTrack};
// use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::native::frame_cryptor::EncryptionState;
use livekit::{E2eeOptions, RoomEvent};
use matrix_sdk::ruma::MilliSecondsSinceUnixEpoch;
use matrix_sdk::ruma::events::call::member::{
    ActiveLivekitFocus, Application, CallApplicationContent, CallMemberEventContent,
    CallMemberStateKey, Focus, LivekitFocus,
};
use matrix_sdk::ruma::events::relation::Reference;
use matrix_sdk::ruma::events::rtc::notification::RtcNotificationEventContent;
use matrix_sdk::ruma::events::{
    AnyStateEventContent, AnyToDeviceEventContent, Mentions, OriginalSyncStateEvent, StateEventType,
};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::{Client, ruma::RoomId};
// use ringbuf::HeapCons;
// use ringbuf::traits::{Consumer, Observer, Producer, Split};
use tokio::sync::{Mutex, RwLock};

// use crate::{LogResultExt, TauriError};
// use shared::api::call::{self, CallEvent};

type PendingUpdates = HashMap<String, Vec<(String, EncryptionKeysEventContent)>>;

static PENDING_KEY_UPDATES: LazyLock<Mutex<PendingUpdates>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
use livekit::Room as LiveKitRoom;

pub type LiveKitRoomManager = Arc<Mutex<HashMap<String, LiveKitRoomData>>>;

#[derive(Debug)]
pub struct LiveKitRoomData {
    pub livekit_room: LiveKitRoom,
    pub cancellation_token: CancellationToken,
    pub key_index: i32,
    pub call_id: Uuid,
}

impl LiveKitRoomData {
    pub fn close_event_stream(&self) -> tokio_util::sync::WaitForCancellationFuture<'_> {
        self.cancellation_token.cancel();
        self.cancellation_token.cancelled()
    }
}

#[tracing::instrument]
pub async fn join_matrixrtc_call(
    matrix_client: Client,
    room_id: String,
    livekit_room_manager: LiveKitRoomManager,
) -> anyhow::Result<()> {
    event!(Level::INFO, "Started joining call");

    let device_id = matrix_client
        .device_id()
        .map(|d| d.to_string())
        .ok_or_else(|| anyhow!("Matrix client is not logged in or missing a device_id"))?;

    let rtc_foci = matrix_client
        .rtc_foci()
        .await
        .map_err(|e| anyhow!("Failed to get RTC foci: {}", e))?;

    let default_livekit_focus_info = rtc_foci
        .iter()
        .find_map(|focus| match focus {
            RtcTransport::LiveKit(info) => Some(info),
            _ => None,
        })
        .ok_or_else(|| anyhow!("No rtc focus information found"))?;

    let jwt_url = default_livekit_focus_info.service_url.clone() + "/sfu/get";

    let openid_token = matrix_sdk::Account::request_openid_token(&matrix_client.account())
        .await
        .map_err(|e| anyhow!("OpenID token request failed: {}", e))?;

    let auth_payload = serde_json::json!({
        "room": room_id,
        "openid_token": {
            "access_token": openid_token.access_token,
            "expires_in": openid_token.expires_in.as_secs(),
            "matrix_server_name": openid_token.matrix_server_name,
            "token_type": openid_token.token_type.to_string(),
        },
        "device_id": device_id
    });

    let http_client = reqwest::Client::new();
    let res = http_client
        .post(&jwt_url)
        .json(&auth_payload)
        .send()
        .await
        .context("Network error")?;

    if !res.status().is_success() {
        let status = res.status();
        let err_body = res.text().await.unwrap_or_default();
        return Err(anyhow!(
            "SFU Server rejected request ({}): {}",
            status,
            err_body
        ));
    }

    let response_json: serde_json::Value = res
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse SFU response JSON: {}", e))?;

    let service_url = response_json["url"].as_str().context("No url returned")?;
    let jwt = response_json["jwt"].as_str().context("No jwt returned")?;

    event!(Level::INFO, "Successfully acquired LiveKit token");

    let mut e2ee_options = None;

    let parsed_room_id = RoomId::parse(&room_id).context("Invalid Room ID format")?;

    let room = matrix_client
        .get_room(&parsed_room_id)
        .context("Room not found or not joined")?;

    let canonical_room_id = room.room_id().to_string();

    let call_content = CallMemberEventContent::new(
        Application::Call(CallApplicationContent::new(
            "".to_string(),
            matrix_sdk::ruma::events::call::member::CallScope::Room,
        )),
        matrix_client
            .device_id()
            .ok_or(anyhow!("No DeviceId"))?
            .into(),
        matrix_sdk::ruma::events::call::member::ActiveFocus::Livekit(ActiveLivekitFocus::new()),
        vec![Focus::Livekit(LivekitFocus::new(
            canonical_room_id.clone(),
            default_livekit_focus_info.service_url.to_string(),
        ))],
        None,
        None,
    );

    let response = room
        .send_state_event_for_key(
            &CallMemberStateKey::new(
                matrix_client.user_id().ok_or(anyhow!("No UserId"))?.into(),
                Some(
                    matrix_client
                        .device_id()
                        .ok_or(anyhow!("No DeviceId"))?
                        .into(),
                ),
                true,
            ),
            call_content,
        )
        .await?;

    event!(
        Level::INFO,
        "Signaled to matrix room that we joined the call"
    );

    let mut notification_event = RtcNotificationEventContent::new(
        MilliSecondsSinceUnixEpoch::now(),
        Duration::from_mins(1),
        matrix_sdk::ruma::events::rtc::notification::NotificationType::Ring,
    );
    notification_event.mentions = Some(Mentions::with_room_mention());
    notification_event.call_intent =
        Some(matrix_sdk::ruma::events::rtc::notification::CallIntent::Audio);
    notification_event.relates_to = Some(Reference::new(response.event_id));

    room.send(notification_event).await?;

    event!(Level::DEBUG, "Sent notification event into matrix room");

    if room.encryption_state().is_encrypted() {
        let key_provider = KeyProvider::new(KeyProviderOptions {
            key_ring_size: 128,
            key_derivation_algorithm: livekit::e2ee::key_provider::KeyDerivationAlgorithm::HKDF,
            ..Default::default()
        });

        e2ee_options = Some(E2eeOptions {
            encryption_type: EncryptionType::Gcm,
            key_provider,
        });
    }

    // LiveKit connection
    let cancellationtoken = CancellationToken::new();
    let mut room_options = livekit::RoomOptions::default();
    room_options.encryption = e2ee_options;

    event!(Level::INFO, "Connecting to LiveKit room");
    let call_id = Uuid::new_v4();
    let (livekit_room, mut event_receiver) =
        livekit::Room::connect(service_url, jwt, room_options).await?;
    event!(Level::INFO, "Connected to LiveKit room: {:?}", livekit_room);

    // set and send out encryption key after joining but before publishing a track
    if room.encryption_state().is_encrypted() {
        event!(Level::DEBUG, "Room is encrypted. Generating encryption key");
        let mut raw_key = [0u8; 16];
        getrandom::fill(&mut raw_key)
            .map_err(|e| anyhow!("Failed to generate cryptographic key: {}", e))?;

        let local_call_key = general_purpose::STANDARD.encode(raw_key);

        let key_provider = livekit_room
            .e2ee_manager()
            .key_provider()
            .expect("Keyprovider already set");
        key_provider.set_key(
            &livekit_room.local_participant().identity(),
            0,
            raw_key.into(),
        );
        event!(Level::DEBUG, "Set encryption key for local participant.");

        event!(
            Level::DEBUG,
            "Trying to send encryption key to call participants"
        );
        // send_encryption_keys(
        //     matrix_client.clone(),
        //     &canonical_room_id,
        //     &local_call_key,
        //     0,
        //     call_id,
        // )
        // .await?;
    }

    livekit_room.e2ee_manager().set_enabled(true);

    let cancellationtoken_clone = cancellationtoken.clone();
    let call_data = LiveKitRoomData {
        livekit_room,
        cancellation_token: cancellationtoken_clone,
        key_index: 0,
        call_id,
    };

    let mut room_manager_guard = livekit_room_manager.lock().await;

    // Store using canonical room ID
    room_manager_guard.insert(canonical_room_id.clone(), call_data);

    // drain encryption keys received before room was set up
    event!(
        Level::DEBUG,
        "Draining encryption keys that arrived before fully joining call"
    );
    let mut pending_map = PENDING_KEY_UPDATES.lock().await;
    let pending_updates = pending_map
        .remove(&canonical_room_id)
        .or_else(|| pending_map.remove(&room_id));

    if let Some(pending_updates) = pending_updates
        && let Some(call_data) = room_manager_guard.get(&canonical_room_id)
    {
        let livekit_room = &call_data.livekit_room;
        let e2ee_manager = livekit_room.e2ee_manager();

        if let Some(key_provider) = e2ee_manager.key_provider() {
            for (sender, update_event) in pending_updates {
                let livekit_id = format!("{}:{}", sender, update_event.member.claimed_device_id);

                match general_purpose::STANDARD.decode(&update_event.keys.key) {
                    Ok(decoded_key) => {
                        let key_index = update_event.keys.index;
                        key_provider.set_key(
                            &From::from(livekit_id.clone()),
                            key_index,
                            decoded_key,
                        );
                        event!(
                            Level::INFO,
                            "Set updated LiveKit decryption key for {} with index {} in KeyProvider.",
                            livekit_id,
                            key_index
                        );

                        e2ee_manager
                            .frame_cryptors()
                            .iter()
                            .filter(|((id, _), _)| id == &From::from(livekit_id.clone()))
                            .for_each(|((id, _), frame_cryptor)| {
                                frame_cryptor.set_key_index(key_index);
                                event!(Level::DEBUG, "Updated FrameCryptor key index for {id}");
                            });
                    }
                    Err(e) => {
                        event!(
                            Level::WARN,
                            "Failed to decode base64 key for participant {livekit_id}: {e}"
                        );
                    }
                }
            }
        } else {
            event!(
                Level::INFO,
                "No key provider found when draining pending key updates.",
            );
        }
    }

    drop(pending_map);
    drop(room_manager_guard);

    // audio setup
    // log::debug!("Setting up audio");
    // if let Some(device) = audio_manager.host.default_output_device() {
    //     if let Err(e) = audio_manager.try_setup_output_stream_for_device(&device) {
    //         warn!("Could not set up output stream: {:?}", e);
    //     } else {
    //         debug!("Set up cpal output stream");
    //     }
    // } else {
    //     warn!("No default output device found");
    // }

    // if let Some(device) = audio_manager.host.default_input_device() {
    //     if let Err(e) = audio_manager.try_setup_input_stream_for_device(&device) {
    //         warn!("Could not set up input stream: {:?}", e);
    //     } else {
    //         debug!("Set up cpal input stream");
    //     }
    // } else {
    //     warn!("No default input device found");
    // }

    // let microphone_track = setup_mic_track(&audio_manager);

    // // publish microphone track
    // if let Ok(track) = microphone_track {
    //     let mut manager = livekit_room_manager.lock().await;
    //     let call_data = manager.get_mut(&canonical_room_id).unwrap();
    //     call_data
    //         .livekit_room
    //         .local_participant()
    //         .publish_track(
    //             LocalTrack::Audio(track),
    //             livekit::options::TrackPublishOptions {
    //                 source: livekit::track::TrackSource::Microphone,
    //                 ..Default::default()
    //             },
    //         )
    //         .await
    //         .map_err(|e| format!("Failed to publish mic: {}", e))?;
    //     debug!("Published microphone track");
    // } else {
    //     warn!(
    //         "Could not set up microphone track: {}",
    //         microphone_track.unwrap_err()
    //     )
    // }

    // let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    // // spawn main audio mixer mixing together the various audio tracks
    // if let Some(producer) = audio_manager.output_producer.lock().unwrap().take() {
    //     log::debug!("Spawning audio mixer");
    //     spawn_audio_mixer(producer, receiver, cancellationtoken.clone());
    // } else {
    //     log::error!("output_producer is None, mixer not spawned, remote audio will be silent");
    // }

    // handle events
    let room_id_clone = canonical_room_id.clone();
    let cancellationtoken_clone = cancellationtoken.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancellationtoken_clone.cancelled() => {
                    event!(Level::INFO, "Background room LiveKit event receiver stopped for room: {}", room_id_clone);
                    break;
                }

                maybe_ev = event_receiver.recv() => {
                    match maybe_ev {
                        Some(ev) => {
                            if let RoomEvent::E2eeStateChanged { participant, state } = ev {
                                // if let Some(encryption_state) = match state {
                                    // EncryptionState::Ok => {Some(call::EncryptionState::Ok)}
                                    // EncryptionState::MissingKey => {Some(call::EncryptionState::MissingKey)}
                                    // EncryptionState::EncryptionFailed => {Some(call::EncryptionState::EncryptionFailed)}
                                    // EncryptionState::DecryptionFailed => {Some(call::EncryptionState::DecryptionFailed)}
                                    // _ => None

                                // } {
                                // let _ = channel.send(CallEvent::EncryptionStateChanged {participant: participant.identity().as_str().into(), state: encryption_state}).log_as_warn();
                                // };
                                event!(Level::DEBUG, "Encryption state changed for {participant:?}, new state: {state:?}");
                            } else if let RoomEvent::TrackSubscribed { track, .. } = ev
                                && let RemoteTrack::Audio(ref audio_track) = track
                            {
                                event!(Level::DEBUG, "Subscribed to new audio track: {:?}", track);
                                // if let Err(e) = register_new_track(handle.clone(), audio_track, sender.clone(), cancellationtoken_clone.clone()).await {
                                    // warn!("Could not register output of remote track: {e}")
                                // }
                            }
                        }
                        None => {
                            event!(Level::INFO, "LiveKit event channel closed by remote host.");
                            break;
                        }
                    }
                }
            }
        }
    });
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EncryptionKeysEventContent {
    pub keys: EncryptionKeysInfo,
    pub member: CallMemberInfo,
    pub room_id: String,
    pub sent_ts: MilliSecondsSinceUnixEpoch,
    pub session: CallSessionInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EncryptionKeysInfo {
    pub index: i32,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CallMemberInfo {
    pub claimed_device_id: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CallSessionInfo {
    pub application: String,
    pub call_id: String,
    pub scope: String,
}

impl Default for CallSessionInfo {
    fn default() -> Self {
        CallSessionInfo {
            application: "m.call".to_string(),
            call_id: "".to_string(),
            scope: "m.room".to_string(),
        }
    }
}

pub(crate) async fn leave_matrixrtc_call(
    matrix_client: Client,
    livekit_room_manager: LiveKitRoomManager,
    room_id: String,
) -> anyhow::Result<()> {
    let parsed_room_id = RoomId::parse(&room_id)?;
    let canonical_room_id = parsed_room_id.to_string();

    // close event stream and remove call from manager
    let mut data_guard = livekit_room_manager.lock().await;
    if let Some(room_data) = data_guard
        .remove(&canonical_room_id)
        .or_else(|| data_guard.remove(&room_id))
    {
        room_data.close_event_stream().await;
        room_data.livekit_room.close().await?;
    } else {
        return Err(anyhow!("Not in a call in this room"));
    }

    let room = matrix_client
        .get_room(&parsed_room_id)
        .context("Room not found or not joined")?;

    let call_content = CallMemberEventContent::new_empty(None);

    let _response = room
        .send_state_event_for_key(
            &CallMemberStateKey::new(
                matrix_client.user_id().context("No UserId")?.into(),
                Some(matrix_client.device_id().context("No DeviceId")?.into()),
                true,
            ),
            call_content,
        )
        .await?;

    Ok(())
}

pub async fn cleanup_ghost_calls(client: &matrix_sdk::Client) {
    let Some(device_id) = client.device_id() else {
        return;
    };
    let Some(user_id) = client.user_id() else {
        return;
    };

    let state_key = format!("_{}_{}", user_id, device_id);

    for room in client.joined_rooms() {
        if let Ok(Some(raw_event)) = room
            .get_state_event(
                matrix_sdk::ruma::events::StateEventType::CallMember,
                &state_key,
            )
            .await
        {
            if let Ok(json_event) = serde_json::to_value(&raw_event)
                && let Some(content) = json_event.get("content")
                && content.as_object().is_some_and(|obj| obj.is_empty())
            {
                continue;
            }

            event!(
                Level::DEBUG,
                "Cleaning up ghost participant in room: {}",
                room.room_id()
            );

            let _ = room
                .send_state_event_raw("m.call.member", &state_key, serde_json::json!({}))
                .await;
        }
    }
}
