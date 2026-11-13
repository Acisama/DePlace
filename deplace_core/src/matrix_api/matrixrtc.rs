// TODO: Remove this
#![allow(unused)]

use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose;
use base64::engine::{DecodePaddingMode, GeneralPurposeConfig};
use futures::StreamExt;
use livekit::e2ee::EncryptionType;
use livekit::e2ee::key_provider::{KeyProvider, KeyProviderOptions};
use livekit::prelude::TrackSid;
use livekit::track::{RemoteAudioTrack, RemoteTrack};
use matrix_sdk::Room;
use matrix_sdk::deserialized_responses::{ProcessedToDeviceEvent, SyncOrStrippedState};
use matrix_sdk::event_handler::Ctx;
use matrix_sdk::ruma::api::client::rtc::{LiveKitRtcTransport, RtcTransport};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
// use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::DePlaceRoom;
use crate::audio::{AudioThread, Microphone};
use crate::call::{
    CallConnectionState, CallEndReason, CallEvent, CallJoinStep, CallParticipantHandle,
    CallParticipantId, CallRegistry, LivekitCall,
};
use crate::member::ProfileStore;
use crate::state::{AppState, UserDevice};
// use cpal::traits::{DeviceTrait, HostTrait};
use livekit::options::TrackPublishOptions;
use livekit::prelude::TrackSource;
use livekit::track::{LocalAudioTrack, LocalTrack};
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::native::frame_cryptor::EncryptionState;
use livekit::webrtc::prelude::AudioFrame;
use livekit::{E2eeOptions, RoomEvent};
use matrix_sdk::ruma::MilliSecondsSinceUnixEpoch;
use matrix_sdk::ruma::events::call::member::{
    ActiveLivekitFocus, Application, CallApplicationContent, CallMemberEventContent,
    CallMemberStateKey, Focus, LivekitFocus,
};
use matrix_sdk::ruma::events::relation::Reference;
use matrix_sdk::ruma::events::rtc::notification::RtcNotificationEventContent;
use matrix_sdk::ruma::events::{
    AnyStateEventContent, AnyToDeviceEventContent, Mentions, OriginalSyncStateEvent,
    StateEventType, SyncStateEvent,
};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::{Client, ruma::RoomId};
// use ringbuf::HeapCons;
// use ringbuf::traits::{Consumer, Observer, Producer, Split};
use tokio::sync::{Mutex, RwLock, watch};

// use crate::{LogResultExt, TauriError};
// use shared::api::call::{self, CallEvent};

/// Keys parked under the empty room id arrived in a dialect without room
/// context and are applied to whichever call joins next.
type PendingUpdates = HashMap<String, Vec<(String, ReceivedEncryptionKeys)>>;

static PENDING_KEY_UPDATES: LazyLock<Mutex<PendingUpdates>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Who was in each room's call when we last looked, so arrivals and
/// departures can be told from sticky membership refreshes.
static LAST_SEEN_CALL_ROSTERS: LazyLock<Mutex<HashMap<String, HashSet<(String, String, ruma::UInt)>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// How long our call key remains fresh enough to hand to an arrival
/// without rotating; older keys are rotated instead.
const KEY_STAYS_FRESH_FOR: Duration = Duration::from_secs(60);

// Perceptual loudness mapping: speech RMS never reaches full scale, so we
// map dBFS between a noise floor and a realistic "loud speech" ceiling.
const VOLUME_FLOOR_DB: f64 = -50.0;
const VOLUME_CEILING_DB: f64 = -12.0;

/// Maps a linear RMS amplitude to perceptual loudness (`0.0..=1.0`),
/// scaling dBFS between [`VOLUME_FLOOR_DB`] and [`VOLUME_CEILING_DB`].
fn rms_to_volume(rms: f64) -> f32 {
    let db = 20.0 * rms.max(1e-5).log10();
    ((db - VOLUME_FLOOR_DB) / (VOLUME_CEILING_DB - VOLUME_FLOOR_DB)).clamp(0.0, 1.0) as f32
}

/// loudness of one received audio frame, normalized to `0.0..=1.0`.
fn frame_volume(frame: &AudioFrame) -> f32 {
    if frame.data.is_empty() {
        return 0.0;
    }

    let sum: f64 = frame
        .data
        .iter()
        .map(|sample| {
            let sample = f64::from(*sample) / f64::from(i16::MAX);
            sample * sample
        })
        .sum();

    rms_to_volume((sum / frame.data.len() as f64).sqrt())
}

// Volume smoothing: rises instantly with the received audio, falls gently
// so meters don't flicker, and decays while no frames arrive at all
const VOLUME_RELEASE: f32 = 0.85;
const VOLUME_SILENCE_DECAY: f32 = 0.7;

/// Forwards everything one remote call track says to the speakers until the
/// call is left or the track goes away, reporting the measured loudness.
fn spawn_remote_audio_pump(
    track: RemoteAudioTrack,
    call: Arc<LivekitCall>,
    participant: CallParticipantId,
    cancellationtoken: CancellationToken,
    audio: AudioThread,
) -> tokio::task::JoinHandle<()> {
    event!(
        Level::DEBUG,
        "Starting to play remote call track {}",
        track.sid()
    );

    // Looked up once, not on every frame
    let handle = call.call_handle(&participant);

    tokio::spawn(async move {
        let player = match audio.frame_player().await {
            Ok(player) => player,
            Err(e) => {
                event!(
                    Level::WARN,
                    "Could not create a player for a call track: {e:#}"
                );
                return;
            }
        };

        let mut stream = NativeAudioStream::new(track.rtc_track(), 48_000, 2);

        // Smoothed loudness of this participant
        let mut level = 0.0_f32;

        loop {
            tokio::select! {
                biased;

                _ = cancellationtoken.cancelled() => break,

                frame = stream.next() => match frame {
                    Some(frame) => {
                        level = frame_volume(&frame).max(level * VOLUME_RELEASE);
                        call.set_participant_volume(&handle, level);
                        // How loud they play for us, whenever the user
                        // changed it in the meantime
                        player.set_volume(handle.output_volume());
                        player.push(&frame);
                    }
                    // Track is done, nothing will ever come again
                    None => break,
                },

                // No frame arrived: decay towards silence, so the meter
                // also falls when the SFU just stops sending frames
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    if level > 0.0 {
                        level *= VOLUME_SILENCE_DECAY;
                        call.set_participant_volume(&handle, level);
                    }
                    player.set_volume(handle.output_volume());
                },
            }
        }

        call.clear_participant_volume(&handle);

        event!(
            Level::DEBUG,
            "Stopped playing remote call track {}",
            track.sid()
        );
    })
}

use livekit::Room as LiveKitRoom;

/// Forwards everything the microphone hears into a LiveKit audio source
/// until cancelled, reporting how loud we are (zero while muted).
fn spawn_microphone_pump(
    microphone: Microphone,
    source: NativeAudioSource,
    call: Arc<LivekitCall>,
    cancellationtoken: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    event!(Level::DEBUG, "Starting to send microphone audio");

    tokio::spawn(async move {
        /// Samples per frame: 10 ms of 48 kHz mono.
        const FRAME_SAMPLES: usize = 480;

        let mut leftover: Vec<f32> = Vec::new();

        loop {
            leftover.extend(microphone.pop());

            while leftover.len() >= FRAME_SAMPLES {
                let frame_volume = if call.is_microphone_muted() {
                    0.0
                } else {
                    let sum: f64 = leftover[..FRAME_SAMPLES]
                        .iter()
                        .map(|sample| {
                            let sample = f64::from(*sample);
                            sample * sample
                        })
                        .sum();
                    rms_to_volume((sum / FRAME_SAMPLES as f64).sqrt())
                };
                call.set_own_volume(frame_volume);

                let data: Vec<i16> = leftover[..FRAME_SAMPLES]
                    .iter()
                    .map(|&sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
                    .collect();
                leftover.drain(..FRAME_SAMPLES);

                let frame = AudioFrame {
                    data: std::borrow::Cow::Owned(data),
                    sample_rate: 48_000,
                    num_channels: 1,
                    samples_per_channel: FRAME_SAMPLES as u32,
                };

                if let Err(e) = source.capture_frame(&frame).await {
                    event!(
                        Level::WARN,
                        "Could not hand a microphone frame to LiveKit: {e:?}"
                    );
                }
            }

            tokio::select! {
                _ = cancellationtoken.cancelled() => break,

                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }

        microphone.clear();
        event!(Level::DEBUG, "Stopped sending microphone audio");
    })
}

pub type LiveKitRoomManager = Arc<Mutex<HashMap<String, LiveKitRoomData>>>;

#[derive(Debug)]
pub struct LiveKitRoomData {
    pub livekit_room: LiveKitRoom,
    pub cancellation_token: CancellationToken,
    pub key_index: i32,
    pub call_id: Uuid,
    /// Our current key and when it was minted; fresh arrivals get it
    /// re-sent instead of a rotation, like Element X does.
    pub current_key: Option<(String, std::time::Instant)>,
    /// The newest key index we received per participant identity, so
    /// freshly subscribed cryptors can be pointed at the right key.
    pub latest_remote_key_indices: HashMap<String, i32>,
}

impl LiveKitRoomData {
    pub fn close_event_stream(&self) -> tokio_util::sync::WaitForCancellationFuture<'_> {
        self.cancellation_token.cancel();
        self.cancellation_token.cancelled()
    }
}

/// Strips trailing slashes off a focus url: everyone joins through
/// `focus_url + "/sfu/get"`, and a trailing slash doubles into a broken one.
fn normalize_focus_url(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}

/// The LiveKit focus url the ongoing call agreed on: the `foci_preferred`
/// of its oldest active membership; any other focus lands us in our own room.
async fn ongoing_call_focus(room: &Room) -> Option<String> {
    let events = room
        .get_state_events_static::<CallMemberEventContent>()
        .await
        .ok()?;

    let mut oldest: Option<(MilliSecondsSinceUnixEpoch, String)> = None;

    for raw in events {
        let Ok(SyncOrStrippedState::Sync(ev)) = raw.deserialize() else {
            continue;
        };
        let SyncStateEvent::Original(ev) = ev else {
            continue;
        };
        let CallMemberEventContent::SessionContent(session) = &ev.content else {
            continue;
        };

        let focus_url = session.foci_preferred.iter().find_map(|focus| match focus {
            Focus::Livekit(focus) => Some(normalize_focus_url(&focus.service_url)),
            _ => None,
        });

        // The oldest membership decides, and its own created_ts is often
        // unset, so the age of its state event carries that meaning here
        if let Some(focus_url) = focus_url
            && oldest
                .as_ref()
                .is_none_or(|(ts, _)| ev.origin_server_ts < *ts)
        {
            oldest = Some((ev.origin_server_ts, focus_url));
        }
    }

    oldest.map(|(_, url)| url)
}

/// Logs the LiveKit identity and room the SFU assigned us: call problems
/// almost always come down to a mismatch here between two clients.
fn log_sfu_assignment(sfu_jwt: &str) {
    if let Some(claims) = sfu_jwt
        .split('.')
        .nth(1)
        .and_then(|payload| general_purpose::URL_SAFE_NO_PAD.decode(payload).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    {
        let identity = claims.get("sub").cloned().unwrap_or_default();
        let livekit_room = claims.pointer("/video/room").cloned().unwrap_or_default();
        event!(
            Level::INFO,
            "The SFU assigned us the LiveKit identity {identity} in the LiveKit room {livekit_room}"
        );
    }
}

pub async fn join_matrixrtc_call(
    room: DePlaceRoom,
    profile_store: Arc<ProfileStore>,
    own_device: Arc<UserDevice>,
    livekit_room_manager: LiveKitRoomManager,
    call_registry: CallRegistry,
    audio: AudioThread,
    state_wake: watch::Sender<()>,
) -> anyhow::Result<Arc<crate::call::LivekitCall>> {
    let progress_room = room.clone();
    let progress_wake = state_wake.clone();

    let result = join_call_in_steps(
        room,
        profile_store,
        own_device,
        livekit_room_manager,
        call_registry,
        audio,
        state_wake,
    )
    .await;

    if result.is_err() {
        progress_room.call.join_progress.set(None);
        progress_wake.send_modify(|_| ());
    }

    result
}

/// Reports how far the join got and wakes the ui about it.
fn report_join_step(room: &DePlaceRoom, state_wake: &watch::Sender<()>, step: CallJoinStep) {
    room.call.join_progress.set(Some(step));
    state_wake.send_modify(|_| ());
}

#[allow(clippy::too_many_arguments)]
async fn join_call_in_steps(
    room: DePlaceRoom,
    profile_store: Arc<ProfileStore>,
    own_device: Arc<UserDevice>,
    livekit_room_manager: LiveKitRoomManager,
    call_registry: CallRegistry,
    audio: AudioThread,
    state_wake: watch::Sender<()>,
) -> anyhow::Result<Arc<crate::call::LivekitCall>> {
    event!(Level::INFO, "Started joining call");

    report_join_step(&room, &state_wake, CallJoinStep::Authenticating);

    let matrix_client = room.room.client();
    let rtc_foci = matrix_client
        .discover_rtc_transports()
        .await
        .map_err(|e| anyhow!("Failed to get RTC foci: {}", e))?
        .unwrap_or_default();

    let default_livekit_focus_info = rtc_foci
        .iter()
        .find_map(|focus| match focus {
            RtcTransport::LiveKit(info) => Some(LiveKitRtcTransport::new(normalize_focus_url(
                &info.service_url,
            ))),
            _ => None,
        })
        .ok_or_else(|| anyhow!("No rtc focus information found"))?;

    // The focus the call actually runs on: whatever discovery returns
    // fresh for us, or whatever an ongoing call already agreed on.
    let livekit_focus_info = match ongoing_call_focus(&room.room).await {
        Some(call_focus) if call_focus != default_livekit_focus_info.service_url => {
            event!(
                Level::INFO,
                "Joining the ongoing call through the focus of its oldest member ({call_focus}) instead of ours ({}), so we land in the same LiveKit room",
                default_livekit_focus_info.service_url
            );
            LiveKitRtcTransport::new(call_focus)
        }
        _ => default_livekit_focus_info.clone(),
    };

    event!(
        Level::INFO,
        "Joining the call through the SFU focus {}",
        livekit_focus_info.service_url
    );

    let jwt_url = livekit_focus_info.service_url.clone() + "/sfu/get";

    let openid_token = matrix_sdk::Account::request_openid_token(&matrix_client.account())
        .await
        .map_err(|e| anyhow!("OpenID token request failed: {}", e))?;

    let room_id = room.room_id();
    let auth_payload = serde_json::json!({
        "room": room_id,
        "openid_token": {
            "access_token": openid_token.access_token,
            "expires_in": openid_token.expires_in.as_secs(),
            "matrix_server_name": openid_token.matrix_server_name,
            "token_type": openid_token.token_type.to_string(),
        },
        "device_id": own_device.device_id
    });

    let http_client = matrix_sdk::reqwest::Client::new();
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

    // Membership event and ringing next, both writes over the network
    report_join_step(&room, &state_wake, CallJoinStep::Signaling);
    log_sfu_assignment(jwt);

    let canonical_room_id = room_id.to_string();
    let canonical_room = room_id.to_owned();

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
            livekit_focus_info.service_url.to_string(),
        ))],
        None,
        None,
    );

    let response = room
        .room
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

    let call = match finish_joining_call(
        &room,
        profile_store,
        own_device.clone(),
        response.event_id,
        service_url.to_string(),
        jwt.to_string(),
        audio,
        livekit_room_manager,
        call_registry,
        state_wake,
    )
    .await
    {
        Ok(call) => call,
        Err(e) => {
            // The membership state event is already out: take it back, or
            // the user appears stuck in a call they cannot leave.
            if let Err(withdraw_error) = withdraw_call_membership(&room.room, &own_device).await {
                event!(
                    Level::ERROR,
                    "Could not withdraw the call membership after failed join: {withdraw_error:#}"
                );
            }

            return Err(e);
        }
    };

    Ok(call)
}

/// Completes a joined call: rings, connects to the SFU, publishes the
/// microphone and spawns the event loop; the membership must be taken back on failure.
#[allow(clippy::too_many_arguments)]
async fn finish_joining_call(
    dp_room: &DePlaceRoom,
    profile_store: Arc<ProfileStore>,
    own_device: Arc<UserDevice>,
    joined_event_id: matrix_sdk::ruma::OwnedEventId,
    service_url: String,
    jwt: String,
    audio: AudioThread,
    livekit_room_manager: LiveKitRoomManager,
    call_registry: CallRegistry,
    state_wake: watch::Sender<()>,
) -> anyhow::Result<Arc<crate::call::LivekitCall>> {
    let mut e2ee_options: Option<E2eeOptions> = None;

    let mut notification_event = RtcNotificationEventContent::new(
        MilliSecondsSinceUnixEpoch::now(),
        Duration::from_mins(1),
        matrix_sdk::ruma::events::rtc::notification::NotificationType::Ring,
    );
    notification_event.mentions = Some(Mentions::with_room_mention());
    notification_event.call_intent =
        Some(matrix_sdk::ruma::events::rtc::notification::CallIntent::Audio);
    notification_event.relates_to = Some(Reference::new(joined_event_id));

    let room = &dp_room.room;
    room.send(notification_event).await?;

    event!(Level::DEBUG, "Sent notification event into matrix room");

    if dp_room.room.encryption_state().is_encrypted() {
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
    report_join_step(dp_room, &state_wake, CallJoinStep::Connecting);
    let call_id = Uuid::new_v4();
    let (livekit_room, mut event_receiver) =
        livekit::Room::connect(&service_url, &jwt, room_options).await?;
    event!(Level::INFO, "Connected to LiveKit room: {:?}", livekit_room);

    // The connection is there, keys and microphone next
    report_join_step(dp_room, &state_wake, CallJoinStep::Preparing);

    // The call handle: the call thread folds its LiveKit events into its
    // state, the frontend reads that state and sends commands to it
    let call = Arc::new(LivekitCall::new(
        dp_room.room.clone(),
        livekit_room_manager.clone(),
        audio.clone(),
        dp_room.call.call_handles.clone(),
        own_device.clone(),
        dp_room.call.own_volume.clone(),
        state_wake,
        dp_room.own_handle.clone(),
        profile_store,
    ));

    // set and send out encryption key after joining but before publishing a track
    // remembered so later arrivals can get it re-sent while fresh
    let mut current_call_key: Option<(String, std::time::Instant)> = None;
    if room.encryption_state().is_encrypted() {
        event!(Level::DEBUG, "Room is encrypted. Generating encryption key");
        let mut raw_key = [0u8; 16];
        getrandom::fill(&mut raw_key)
            .map_err(|e| anyhow!("Failed to generate cryptographic key: {}", e))?;

        let local_call_key = general_purpose::STANDARD.encode(raw_key);
        current_call_key = Some((local_call_key.clone(), std::time::Instant::now()));

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
        send_encryption_keys(room, &own_device, &local_call_key, 0, call_id).await?;
    }

    livekit_room.e2ee_manager().set_enabled(true);

    // publish the microphone track so the other participants can hear us
    match audio.microphone().await {
        Ok(microphone) => {
            let source = NativeAudioSource::new(
                AudioSourceOptions {
                    echo_cancellation: true,
                    noise_suppression: true,
                    auto_gain_control: true,
                },
                48_000,
                1,
                1_000,
            );

            let microphone_track = LocalAudioTrack::create_audio_track(
                "microphone",
                RtcAudioSource::Native(source.clone()),
            );

            let participant = livekit_room.local_participant();
            let publish = participant.publish_track(
                LocalTrack::Audio(microphone_track.clone()),
                TrackPublishOptions {
                    source: TrackSource::Microphone,
                    ..Default::default()
                },
            );

            match publish.await {
                Ok(_) => {
                    event!(Level::INFO, "Published the microphone track");
                    call.set_microphone(microphone_track);
                    spawn_microphone_pump(
                        microphone,
                        source,
                        call.clone(),
                        cancellationtoken.clone(),
                    );
                }
                Err(e) => {
                    // The call continues without a microphone if publishing fails
                    event!(Level::WARN, "Could not publish the microphone track: {e}");
                }
            }
        }
        Err(e) => {
            event!(Level::WARN, "Joining the call without a microphone: {e:#}");
        }
    }

    let cancellationtoken_clone = cancellationtoken.clone();
    let call_data = LiveKitRoomData {
        livekit_room,
        cancellation_token: cancellationtoken_clone,
        key_index: 0,
        call_id,
        latest_remote_key_indices: HashMap::new(),
        current_key: current_call_key,
    };

    let mut room_manager_guard = livekit_room_manager.lock().await;

    let room_id_str = room.room_id().to_string();

    // Store using canonical room ID
    room_manager_guard.insert(room_id_str.clone(), call_data);
    drop(room_manager_guard);

    // Drain encryption keys received before fully joining the call
    event!(
        Level::DEBUG,
        "Draining encryption keys that arrived before fully joining call"
    );
    let pending_updates = {
        let mut pending_map = PENDING_KEY_UPDATES.lock().await;
        let mut drained = pending_map.remove(&room_id_str).unwrap_or_default();
        // Keys that arrived without room context belong to whichever call
        // joined last.
        drained.extend(pending_map.remove("").unwrap_or_default());
        drained
    };

    for (sender, content) in pending_updates {
        apply_remote_key(&sender, &content, &livekit_room_manager).await;
    }

    // The call is live: hand the frontend its handle
    call_registry.insert(call.clone());
    // Set last: every `?` above would otherwise leave a call behind whose
    // event loop never started
    dp_room.call.active.set(Some(call.clone()));

    // The join is done, nothing to show progress about anymore
    dp_room.call.join_progress.set(None);
    call.wake();

    // handle events
    let cancellationtoken_clone = cancellationtoken.clone();
    let audio_clone = audio.clone();
    let call_loop = call.clone();
    let call_registry_loop = call_registry.clone();
    let room_call_loop = dp_room.call.clone();
    let room_manager_loop = livekit_room_manager.clone();
    tokio::spawn(async move {
        // One pump per remote audio track, so their audio and measured
        // volume go away again when the track does
        let mut audio_pumps: HashMap<
            TrackSid,
            (CallParticipantHandle, tokio::task::JoinHandle<()>),
        > = HashMap::new();

        // How the call ended, left on purpose counts as Left, anything
        // else (channel closed, SFU disconnect) as Disconnected.
        let mut end_reason = CallEndReason::Disconnected;

        loop {
            tokio::select! {
                _ = cancellationtoken_clone.cancelled() => {
                    event!(Level::INFO, "Background room LiveKit event receiver stopped for room: {}", room_id_str);
                    break;
                }

                maybe_ev = event_receiver.recv() => {
                    match maybe_ev {
                        Some(ev) => {
                            // Fold every event into the call state so the
                            // frontend sees everything happen live
                            call_loop.handle_room_event(&ev);

                            if let RoomEvent::Disconnected { .. } = ev {
                                event!(Level::INFO, "Disconnected from the LiveKit room");
                                break;
                            } else if let RoomEvent::TrackSubscribed {
                                track: RemoteTrack::Audio(audio_track),
                                participant,
                                ..
                            } = &ev
                            {
                                event!(Level::DEBUG, "Subscribed to new audio track: {:?} from {:?}", audio_track, participant);
                                let participant_id =
                                    CallParticipantId::from_identity(&participant.identity());
                                let handle = call_loop.call_handle(&participant_id);

                                // New cryptors start at key index 0: point them
                                // at the newest key index we have for them.
                                {
                                    let guard = room_manager_loop.lock().await;
                                    if let Some(call_data) = guard.get(&room_id_str) {
                                        let livekit_id = participant.identity().to_string();
                                        if let Some(key_index) = call_data
                                            .latest_remote_key_indices
                                            .get(&livekit_id)
                                        {
                                            call_data
                                                .livekit_room
                                                .e2ee_manager()
                                                .frame_cryptors()
                                                .iter()
                                                .filter(|((id, _), _)| {
                                                    *id == From::from(livekit_id.clone())
                                                })
                                                .for_each(|((id, _), frame_cryptor)| {
                                                    frame_cryptor.set_key_index(*key_index);
                                                    event!(
                                                        Level::DEBUG,
                                                        "Updated FrameCryptor key index for new track of {id} to {key_index}"
                                                    );
                                                });
                                        }
                                    }
                                }

                                let pump = spawn_remote_audio_pump(
                                    audio_track.clone(),
                                    call_loop.clone(),
                                    participant_id,
                                    cancellationtoken_clone.clone(),
                                    audio_clone.clone(),
                                );
                                audio_pumps.insert(audio_track.sid(), (handle, pump));
                            }
                            else if let RoomEvent::TrackUnsubscribed {
                                track: RemoteTrack::Audio(audio_track),
                                ..
                            } = &ev
                            {
                                event!(Level::DEBUG, "Unsubscribed from audio track: {:?}", audio_track);
                                if let Some((handle, pump)) =
                                    audio_pumps.remove(&audio_track.sid())
                                {
                                    call_loop.clear_participant_volume(&handle);
                                    pump.abort();
                                }
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

        // Whatever is still playing stops with the call, taking its
        // measured volume with it
        for (_, (handle, pump)) in audio_pumps {
            call_loop.clear_participant_volume(&handle);
            pump.abort();
        }

        // We're out of the call: nobody's LiveKit state is known anymore
        call_loop.reset_participants();

        // Signal the end of the call and retire the frontend's handle
        call_loop.change_own_connection(CallConnectionState::Ended);
        call_loop.emit(CallEvent::Ended { reason: end_reason });
        call_registry_loop.remove(call_loop.room_id());
        // Only clear our own call, a rejoin may already have replaced it
        room_call_loop.active.change(|active| {
            if active.as_ref().is_some_and(|a| Arc::ptr_eq(a, &call_loop)) {
                *active = None;
            }
        });
    });

    Ok(call)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CallSessionInfo {
    pub application: String,
    pub call_id: String,
    pub scope: String,
}

/// A call encryption key event as it arrived, in whichever dialect the
/// sender speaks; see [`parse_encryption_keys_event`].
#[derive(Debug, Clone)]
struct ReceivedEncryptionKeys {
    /// The media keys as they arrived: (index, base64 encoded key) pairs.
    keys: Vec<(i32, String)>,
    /// The sender's `member.claimed_device_id`, or the legacy `device_id`.
    claimed_device_id: Option<String>,
    /// The room the key belongs to; `None` in dialects without one.
    room_id: Option<String>,
}

/// Reads the content of an `io.element.call.encryption_keys` to-device
/// event in any of the dialects in the wild, or `None` if it matches none.
fn parse_encryption_keys_event(
    content: &serde_json::Value,
) -> Option<ReceivedEncryptionKeys> {
    /// Reads a single `{ index, key }` pair.
    fn parse_key(value: &serde_json::Value) -> Option<(i32, String)> {
        Some((
            value.get("index")?.as_i64()? as i32,
            value.get("key")?.as_str()?.to_owned(),
        ))
    }

    let keys = match content.get("keys")? {
        // A single key, the shape DePlace and Element Call send
        serde_json::Value::Object(_) => vec![parse_key(content.get("keys")?)?],
        // An array of keys, the shape Element X and older clients send
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(parse_key)
            .collect::<Vec<(i32, String)>>(),
        _ => return None,
    };
    if keys.is_empty() {
        return None;
    }

    let claimed_device_id = content
        .pointer("/member/claimed_device_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| content.get("device_id").and_then(serde_json::Value::as_str))
        .map(str::to_owned);
    let room_id = content
        .get("room_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    Some(ReceivedEncryptionKeys {
        keys,
        claimed_device_id,
        room_id,
    })
}

/// Decodes a media key: MSC4143 prescribes unpadded base64, Element's older
/// dialects send padded, so accept both.
fn decode_media_key(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    const MEDIA_KEY_BASE64: general_purpose::GeneralPurpose =
        general_purpose::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
        );
    MEDIA_KEY_BASE64.decode(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_deplace_dialect() {
        let content = serde_json::json!({
            "keys": { "index": 4, "key": "cGFkZGVk" },
            "member": { "claimed_device_id": "ABCDEF", "id": "call" },
            "room_id": "!room:server",
            "session": { "application": "m.call", "call_id": "", "scope": "m.room" },
            "sent_ts": 123
        });

        let parsed = parse_encryption_keys_event(&content).expect("parses");
        assert_eq!(
            parsed.claimed_device_id.as_deref(),
            Some("ABCDEF"),
            "the member's device id is used"
        );
        assert_eq!(parsed.room_id.as_deref(), Some("!room:server"));
        assert_eq!(parsed.keys, vec![(4, "cGFkZGVk".to_owned())]);
    }

    #[test]
    fn parses_the_legacy_dialect_with_key_array_and_top_level_device() {
        let content = serde_json::json!({
            "keys": [
                { "index": 0, "key": "dW5wYWRkZWQ" },
                { "index": 1, "key": "cGFkZGVkMDA=" }
            ],
            "device_id": "GHIJKL",
            "sent_ts": 456
        });

        let parsed = parse_encryption_keys_event(&content).expect("parses");
        assert_eq!(
            parsed.claimed_device_id.as_deref(),
            Some("GHIJKL"),
            "falls back to the top level device id"
        );
        assert_eq!(parsed.room_id, None, "the legacy dialect has no room");
        assert_eq!(parsed.keys.len(), 2);
        assert_eq!(parsed.keys[0], (0, "dW5wYWRkZWQ".to_owned()));

        // Both arrive in the same message, so both encodings must decode:
        // "dW5wYWRkZWQ" is "unpadded", "cGFkZGVkMDA=" is padded.
        assert_eq!(
            decode_media_key("dW5wYWRkZWQ").as_deref(),
            Ok::<&[u8], &base64::DecodeError>(b"unpadded")
        );
        assert_eq!(
            decode_media_key("cGFkZGVkMDA=").as_deref(),
            Ok::<&[u8], &base64::DecodeError>(b"padded00")
        );
    }

    #[test]
    fn rejects_events_without_keys() {
        assert!(parse_encryption_keys_event(&serde_json::json!({})).is_none());
        assert!(
            parse_encryption_keys_event(&serde_json::json!({ "keys": [] })).is_none(),
            "an empty key array carries nothing to apply"
        );
        assert!(
            parse_encryption_keys_event(&serde_json::json!({ "keys": "nonsense" })).is_none()
        );
    }

    #[test]
    fn the_outbound_payload_speaks_every_dialect() {
        // "a mediation key" in padded base64
        let key = "YSBtZWRpYXRpb24ga2V5";
        let payload = build_encryption_keys_payload(
            "@alice:example.io",
            "V5cP8FErcB",
            "!room:example.io",
            key,
            0,
            Uuid::nil(),
        );

        // Our own reader understands it
        let parsed = parse_encryption_keys_event(&payload).expect("parses");
        assert_eq!(
            parsed.claimed_device_id.as_deref(),
            Some("V5cP8FErcB"),
            "readers preferring the claimed device find it"
        );
        assert_eq!(parsed.room_id.as_deref(), Some("!room:example.io"));
        assert_eq!(parsed.keys, vec![(0, key.to_owned())]);

        // Readers binding the key to member.id, like Element X does, get a
        // real LiveKit identity, not something meaningless like a call id
        assert_eq!(
            payload.pointer("/member/id").and_then(serde_json::Value::as_str),
            Some("@alice:example.io:V5cP8FErcB")
        );

        // The pre-sticky dialects read the sender's device at the top level
        assert_eq!(
            payload.get("device_id").and_then(serde_json::Value::as_str),
            Some("V5cP8FErcB")
        );
    }
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

/// Applies one received call encryption key to the LiveKit room's key
/// provider and cryptors; parks it if we are not in that call (yet).
async fn apply_remote_key(
    sender: &str,
    received: &ReceivedEncryptionKeys,
    livekit_room_manager: &LiveKitRoomManager,
) {
    // Without a device to attribute the key to there is no LiveKit identity
    // to file it under, so it is useless to us.
    let Some(claimed_device_id) = received.claimed_device_id.as_deref() else {
        event!(
            Level::WARN,
            "Received a call encryption key from {sender} without a device id, ignoring it"
        );
        return;
    };

    let livekit_id = format!("{}:{}", sender, claimed_device_id);

    {
        let mut guard = livekit_room_manager.lock().await;

        // Dialects without room context are applied to every call we are
        // in: a key under the wrong room is simply never used.
        let mut applied = false;
        let applicable_rooms: Vec<String> = match &received.room_id {
            Some(room_id) => guard.contains_key(room_id).then(|| vec![room_id.clone()]).unwrap_or_default(),
            None => guard.keys().cloned().collect(),
        };

        for room_id in applicable_rooms {
            let Some(call_data) = guard.get_mut(&room_id) else {
                continue;
            };
            applied = true;

            let e2ee_manager = call_data.livekit_room.e2ee_manager();

            let Some(key_provider) = e2ee_manager.key_provider() else {
                event!(
                    Level::WARN,
                    "No key provider found while applying a call key from {livekit_id}"
                );
                continue;
            };

            for (key_index, key) in &received.keys {
                match decode_media_key(key) {
                    Ok(decoded_key) => {
                        key_provider.set_key(
                            &From::from(livekit_id.clone()),
                            *key_index,
                            decoded_key,
                        );
                        call_data
                            .latest_remote_key_indices
                            .insert(livekit_id.clone(), *key_index);
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
                                frame_cryptor.set_key_index(*key_index);
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
        }

        // Every call the key could belong to was handled; when the key
        // could not be applied anywhere, park it below.
        if applied {
            return;
        }
    }

    event!(
        Level::INFO,
        "Received a call encryption key from {livekit_id} but there is no call for it yet; parking it"
    );
    PENDING_KEY_UPDATES
        .lock()
        .await
        .entry(received.room_id.clone().unwrap_or_default())
        .or_default()
        .push((sender.to_string(), received.clone()));
}

/// Builds our call key to-device content in Element Call's dialect, with
/// `member.id` as the LiveKit id (`user:device`) everyone parseable binds to.
fn build_encryption_keys_payload(
    own_user_id: &str,
    own_device_id: &str,
    room_id: &str,
    key: &str,
    index: i32,
    call_id: Uuid,
) -> serde_json::Value {
    serde_json::json!({
        "room_id": room_id,
        "member": {
            "claimed_device_id": own_device_id,
            "id": format!("{}:{}", own_user_id, own_device_id),
        },
        // The pre-sticky dialects read the device here instead
        "device_id": own_device_id,
        "keys": {
            "index": index,
            "key": key,
        },
        "session": CallSessionInfo {
            application: "m.call".to_string(),
            call_id: call_id.to_string(),
            scope: "m.room".to_string(),
        },
        "sent_ts": MilliSecondsSinceUnixEpoch::now().get(),
    })
}

/// Sends one of our call encryption keys to every other device in the call
/// via per-device encrypted to-device events, the way Element does and god intended
async fn send_encryption_keys(
    room: &Room,
    own_device: &UserDevice,
    key: &str,
    index: i32,
    call_id: Uuid,
) -> anyhow::Result<()> {
    event!(
        Level::DEBUG,
        "Distributing call encryption key to the other participants"
    );

    let state_events = room
        .get_state_events_static::<CallMemberEventContent>()
        .await?;

    let mut recipients = Vec::new();
    let client = room.client();

    for raw in state_events {
        let Ok(SyncOrStrippedState::Sync(ev)) = raw.deserialize() else {
            continue;
        };
        let SyncStateEvent::Original(ev) = ev else {
            continue;
        };

        for membership in ev.content.active_memberships(Some(ev.origin_server_ts)) {
            let user = ev.state_key.user_id();
            let device_id = membership.device_id();

            if user == own_device.user_id && device_id == own_device.device_id {
                continue;
            }

            match client.encryption().get_device(user, device_id).await {
                Ok(Some(device)) => recipients.push(device),
                Ok(None) => event!(
                    Level::INFO,
                    "{device_id} of {user} is in the call but unknown to us, skipping it in call key distribution"
                ),
                Err(e) => event!(
                    Level::WARN,
                    "Could not look up {device_id} of {user} for call key distribution: {e}"
                ),
            }
        }
    }

    if recipients.is_empty() {
        event!(
            Level::INFO,
            "No other call participants to send an encryption key to"
        );
        return Ok(());
    }

    // The shape of Element Call's current dialect, which every reader in the
    // wild parses, see [`build_encryption_keys_payload`].
    let payload = build_encryption_keys_payload(
        &own_device.user_id.to_string(),
        &own_device.device_id.to_string(),
        &room.room_id().to_string(),
        key,
        index,
        call_id,
    );

    let raw_payload: Raw<AnyToDeviceEventContent> =
        serde_json::from_str(&serde_json::to_string(&payload)?)?;

    let recipient_count = recipients.len();
    let failures = room
        .client()
        .encryption()
        .encrypt_and_send_raw_to_device(
            recipients.iter().collect(),
            "io.element.call.encryption_keys",
            raw_payload,
            Default::default(),
        )
        .await?;

    if failures.is_empty() {
        event!(
            Level::INFO,
            "Finished distributing the call encryption key to {recipient_count} device(s)"
        );
    } else {
        event!(
            Level::WARN,
            "Could not send the call encryption key to {} of {recipient_count} device(s)",
            failures.len()
        );
    }

    Ok(())
}

/// Applies the call encryption keys from a sync batch's per-device
/// encrypted `io.element.call.encryption_keys` to-device events.
pub async fn handle_to_device_messages(
    events: Vec<ProcessedToDeviceEvent>,
    livekit_room_manager: LiveKitRoomManager,
) {
    for event in events {
        let ProcessedToDeviceEvent::Decrypted { raw, .. } = event else {
            continue;
        };

        let event_type = raw
            .get_field::<String>("type")
            .ok()
            .flatten()
            .unwrap_or_default();

        if event_type != "io.element.call.encryption_keys" {
            continue;
        }

        let Some(sender) = raw.get_field::<String>("sender").ok().flatten() else {
            event!(
                Level::WARN,
                "Received a call key event without a sender, ignoring it"
            );
            continue;
        };

        let Some(content) = raw
            .deserialize_as::<serde_json::Value>()
            .ok()
            .and_then(|value| value.get("content").cloned())
        else {
            event!(
                Level::WARN,
                "Received a call key event without content, ignoring it"
            );
            continue;
        };

        let Some(content) = parse_encryption_keys_event(&content) else {
            event!(
                Level::WARN,
                "Received a call encryption key event in a dialect we don't know"
            );
            event!(Level::DEBUG, "Its content: {content}");
            continue;
        };

        apply_remote_key(&sender, &content, &livekit_room_manager).await;
    }
}

/// Every device currently in a room's call, as (user id, device id, when
/// the membership was created); the timestamp tells joins from refreshes.
async fn call_roster(room: &matrix_sdk::Room) -> anyhow::Result<HashSet<(String, String, ruma::UInt)>> {
    let state_events = room
        .get_state_events_static::<CallMemberEventContent>()
        .await?;

    let mut roster = HashSet::new();

    for raw in state_events {
        let Ok(SyncOrStrippedState::Sync(ev)) = raw.deserialize() else {
            continue;
        };
        let SyncStateEvent::Original(ev) = ev else {
            continue;
        };

        for membership in ev.content.active_memberships(Some(ev.origin_server_ts)) {
            // created_ts is carried over on refreshes and new on joins;
            // a missing one falls back to the event's own timestamp.
            let created_ts = membership
                .created_ts()
                .unwrap_or(ev.origin_server_ts)
                .get();
            roster.insert((
                ev.state_key.user_id().to_string(),
                membership.device_id().to_string(),
                created_ts,
            ));
        }
    }

    Ok(roster)
}

/// Rolls and distributes our call key when the roster changes and we are
/// in the call of an encrypted room; unchanged rosters are absorbed.
pub async fn handle_call_member_change(
    _ev: OriginalSyncStateEvent<CallMemberEventContent>,
    room: matrix_sdk::Room,
    state: Ctx<AppState>,
) {
    if !room.encryption_state().is_encrypted() {
        return;
    }

    let livekit_room_manager = state.livekit_room_manager();

    // A departure rotates the key, an arrival gets the current one
    // re-sent; the first look at a call only establishes the baseline.
    let (arrivals, departures) = {
        let Ok(roster) = call_roster(&room).await else {
            event!(
                Level::WARN,
                "Could not read the call roster, not touching the call key"
            );
            return;
        };

        let mut last_rosters = LAST_SEEN_CALL_ROSTERS.lock().await;
        let previous = last_rosters.insert(room.room_id().to_string(), roster.clone());
        match previous {
            None => {
                event!(
                    Level::DEBUG,
                    "Taking the current call roster as the baseline, not touching the call key"
                );
                (Vec::new(), Vec::new())
            }
            Some(previous) => {
                let arrivals: Vec<_> = roster.difference(&previous).cloned().collect();
                let departures: Vec<_> = previous.difference(&roster).cloned().collect();
                (arrivals, departures)
            }
        }
    };

    if arrivals.is_empty() && departures.is_empty() {
        event!(
            Level::DEBUG,
            "The call roster did not change, not touching the call key"
        );
        return;
    }

    // A pure arrival with a fresh key just gets it re-sent, like
    // Element X; departures and stale keys fall back to rotating.
    let mut resend_current_key = false;
    if departures.is_empty() {
        let guard = livekit_room_manager.lock().await;
        if let Some(call_data) = guard.get(room.room_id().as_str()) {
            if let Some((_, generated_at)) = &call_data.current_key {
                if generated_at.elapsed() < KEY_STAYS_FRESH_FOR {
                    resend_current_key = true;
                } else {
                    event!(
                        Level::DEBUG,
                        "Arrival with an old call key: rotating instead of re-sending"
                    );
                }
            }
        }
        // No call data or no current key: fall through to the rotation path,
        // which re-reads everything under lock and handles absence.
        if resend_current_key {
            drop(guard);

            let (key, key_index, call_id) = {
                let mut guard = livekit_room_manager.lock().await;
                let Some(call_data) = guard.get(room.room_id().as_str()) else {
                    return;
                };
                let Some((key, _)) = call_data.current_key.clone() else {
                    return;
                };
                (key, call_data.key_index, call_data.call_id)
            };

            event!(
                Level::DEBUG,
                "Someone joined the call, re-sending the current call encryption key"
            );
            let client = state.client();
            if let Err(e) = send_encryption_keys(
                &room,
                &state.inner.own_device,
                &key,
                key_index,
                call_id,
            )
            .await
            {
                event!(Level::ERROR, "Could not re-send the call encryption key: {e}");
            }
            return;
        }
    }

    let (client, key, key_index, call_id) = {
        let mut guard = livekit_room_manager.lock().await;

        let Some(call_data) = guard.get_mut(room.room_id().as_str()) else {
            // We are not in the call, nothing to roll
            return;
        };

        let mut raw_key = [0u8; 16];
        if let Err(e) = getrandom::fill(&mut raw_key) {
            event!(Level::ERROR, "Could not roll the call encryption key: {e}");
            return;
        }
        let key = general_purpose::STANDARD.encode(raw_key);

        call_data.key_index += 1;
        call_data.current_key = Some((key.clone(), std::time::Instant::now()));
        let key_index = call_data.key_index;

        let Some(key_provider) = call_data.livekit_room.e2ee_manager().key_provider() else {
            event!(
                Level::ERROR,
                "Encrypted call room without a key provider, could not roll the key"
            );
            return;
        };
        let local_identity = call_data.livekit_room.local_participant().identity();
        key_provider.set_key(&local_identity, key_index, raw_key.into());

        let e2ee_manager = call_data.livekit_room.e2ee_manager();
        let all_cryptors = e2ee_manager.frame_cryptors();
        let local_cryptors: Vec<_> = all_cryptors
            .iter()
            .filter(|((id, _), _)| id == &local_identity)
            .collect();
        if local_cryptors.is_empty() {
            event!(
                Level::WARN,
                "No frame cryptors found for the local participant while rolling the call key"
            );
        } else {
            local_cryptors
                .iter()
                .for_each(|((_, track), frame_cryptor)| {
                    frame_cryptor.set_key_index(key_index);
                    event!(
                        Level::DEBUG,
                        "Updated local FrameCryptor key index for track {track}"
                    );
                });
        }

        event!(
            Level::INFO,
            "Rolled our call encryption key, now at index {key_index}"
        );

        (state.client(), key, key_index, call_data.call_id)
    };

    if let Err(e) =
        send_encryption_keys(&room, &state.inner.own_device, &key, key_index, call_id).await
    {
        event!(
            Level::ERROR,
            "Could not distribute the rolled call encryption key: {e:#}"
        );
    }
}

/// Withdraws our device's MatrixRTC membership in a room, removing us
/// from the call participants other clients see there.
async fn withdraw_call_membership(room: &Room, own_device: &UserDevice) -> anyhow::Result<()> {
    let call_content = CallMemberEventContent::new_empty(None);

    let _response = room
        .send_state_event_for_key(
            &CallMemberStateKey::new(
                own_device.user_id.clone(),
                Some(own_device.device_id.to_string()),
                true,
            ),
            call_content,
        )
        .await?;

    Ok(())
}

pub async fn leave_matrixrtc_call(
    room: &Room,
    own_device: &UserDevice,
    livekit_room_manager: LiveKitRoomManager,
) -> anyhow::Result<()> {
    let room_id = room.room_id().to_string();

    // close event stream and remove call from manager
    let mut data_guard = livekit_room_manager.lock().await;
    if let Some(room_data) = data_guard.remove(&room_id) {
        room_data.close_event_stream().await;
        room_data.livekit_room.close().await?;
    } else {
        // No connected call on this device: our leftover membership is
        // still flagging us into the call, so withdraw it.
        event!(
            Level::WARN,
            "No connected call in {room_id} to close, withdrawing the membership anyway"
        );
    }

    withdraw_call_membership(room, own_device).await
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

            if let Err(e) = room
                .send_state_event_raw("m.call.member", &state_key, serde_json::json!({}))
                .await
            {
                tracing::error!("Failed to send state event: {}", e);
            };
        }
    }
}
