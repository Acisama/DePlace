// TODO: Remove this
#![allow(unused)]

use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose;
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
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
// use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::DePlaceRoom;
use crate::audio::{AudioThread, Microphone};
use crate::call::{
    CallConnectionState, CallEndReason, CallEvent, CallParticipantHandle, CallParticipantId,
    CallRegistry, LivekitCall,
};
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

type PendingUpdates = HashMap<String, Vec<(String, EncryptionKeysEventContent)>>;

static PENDING_KEY_UPDATES: LazyLock<Mutex<PendingUpdates>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// Perceptual loudness mapping: speech RMS almost never reaches full scale,
// so we map dBFS between a noise floor and a realistic "loud speech" ceiling
// instead of a linear amplitude range.
const VOLUME_FLOOR_DB: f64 = -50.0;
const VOLUME_CEILING_DB: f64 = -12.0;

/// Maps a linear RMS amplitude (`0.0..=1.0` scale) to perceptual loudness,
/// normalized to `0.0..=1.0`, by converting to dBFS and scaling between
/// [`VOLUME_FLOOR_DB`] and [`VOLUME_CEILING_DB`].
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

/// Forwards everything one remote call track says to the speakers. The
/// task stops when the call is left or the track
/// goes away, whichever happens first.
///
/// While pumping, it measures how loud the participant is from the audio
/// actually received and reports that to the call
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

/// Forwards everything the microphone hears into a LiveKit audio source until cancelled.
///
/// While pumping, it also reports how loud our own microphone is (gated
/// to zero while muted) to the call
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
}

impl LiveKitRoomData {
    pub fn close_event_stream(&self) -> tokio_util::sync::WaitForCancellationFuture<'_> {
        self.cancellation_token.cancel();
        self.cancellation_token.cancelled()
    }
}

/// Strips trailing slashes off a focus service url since every client joins
/// through the focus as `focus_url + "/sfu/get"`, so a
/// trailing slash in the foci we advertise turns into a double slash in
/// theirs. This is horrible and kills other people
fn normalize_focus_url(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}

/// The LiveKit focus url the ongoing call in this room agreed on: the
/// `foci_preferred` of its oldest active membership, which every
/// `oldest_membership` client follows when joining. `None` when no call
/// is ongoing or its memberships carry no LiveKit focus.
///
/// The focus decides which SFU is used for our jwt, and the SFU derives the
/// LiveKit room from it. Joining through any other focus  lands us in a
/// room of our own: silent to the whole call both ways, with everyone
/// showing everyone else as waiting for media. The same foci we write
/// into our own membership carry it on to whoever joins after us.
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

/// Logs the LiveKit identity and room the SFU assigned us in its jwt.
///
/// Call problems between clients almost always come down to one side
/// ending up under a different identity or room than the others expect,
/// and this line is the first thing to compare between them.
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
    own_device: Arc<UserDevice>,
    livekit_room_manager: LiveKitRoomManager,
    call_registry: CallRegistry,
    audio: AudioThread,
    state_wake: watch::Sender<()>,
) -> anyhow::Result<Arc<crate::call::LivekitCall>> {
    event!(Level::INFO, "Started joining call");

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

/// Completes a joined call: rings the room, connects to the SFU, publishes
/// the microphone and spawns the event loop driving the call handle.
///
/// Runs after our MatrixRTC membership is already flagged in the room,
/// the caller is responsible for taking that membership back when joining
/// fails from here on.
#[allow(clippy::too_many_arguments)]
async fn finish_joining_call(
    dp_room: &DePlaceRoom,
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
    let call_id = Uuid::new_v4();
    let (livekit_room, mut event_receiver) =
        livekit::Room::connect(&service_url, &jwt, room_options).await?;
    event!(Level::INFO, "Connected to LiveKit room: {:?}", livekit_room);

    // The call handle: the call thread folds its LiveKit events into its
    // state, the frontend reads that state and sends commands to it
    let call = Arc::new(LivekitCall::new(
        dp_room.room.clone(),
        livekit_room_manager.clone(),
        dp_room.call.call_handles.clone(),
        own_device.clone(),
        dp_room.call.own_volume.clone(),
        state_wake,
        dp_room.own_handle.clone(),
    ));

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
        pending_map
            .remove(&room_id_str)
            .or_else(|| pending_map.remove(&room_id_str))
    };

    if let Some(pending_updates) = pending_updates {
        for (sender, content) in pending_updates {
            apply_remote_key(&sender, &content, &livekit_room_manager).await;
        }
    }

    // The call is live: hand the frontend its handle
    call_registry.insert(call.clone());
    // Set last: every `?` above would otherwise leave a call behind whose
    // event loop never started
    dp_room.call.active.set(Some(call.clone()));

    // handle events
    let cancellationtoken_clone = cancellationtoken.clone();
    let audio_clone = audio.clone();
    let call_loop = call.clone();
    let call_registry_loop = call_registry.clone();
    let room_call_loop = dp_room.call.clone();
    tokio::spawn(async move {
        // One pump per remote audio track, so tracks can go away again
        // without their audio playing forever, remembered together
        // with whose voice it carries, so the measured volume clears
        // when it goes
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
                            // frontend sees speaking, mute, join, leave
                            // and encryption changes
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

/// Applies one call encryption key received from another participant to
/// the LiveKit room: into the key provider and the sender's frame cryptors,
/// so their audio can be decrypted.
///
/// When we are not in that room's call (yet), the key is parked in
/// [`PENDING_KEY_UPDATES`] until joining drains it.
async fn apply_remote_key(
    sender: &str,
    content: &EncryptionKeysEventContent,
    livekit_room_manager: &LiveKitRoomManager,
) {
    let livekit_id = format!("{}:{}", sender, content.member.claimed_device_id);

    {
        let guard = livekit_room_manager.lock().await;

        if let Some(call_data) = guard.get(&content.room_id) {
            let e2ee_manager = call_data.livekit_room.e2ee_manager();

            let Some(key_provider) = e2ee_manager.key_provider() else {
                event!(
                    Level::WARN,
                    "No key provider found while applying a call key from {livekit_id}"
                );
                return;
            };

            match general_purpose::STANDARD.decode(&content.keys.key) {
                Ok(decoded_key) => {
                    let key_index = content.keys.index;
                    key_provider.set_key(&From::from(livekit_id.clone()), key_index, decoded_key);
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

            return;
        }
    }

    event!(
        Level::INFO,
        "Received a call encryption key from {livekit_id} but there is no call in {} yet; parking it",
        content.room_id
    );
    PENDING_KEY_UPDATES
        .lock()
        .await
        .entry(content.room_id.clone())
        .or_default()
        .push((sender.to_string(), content.clone()));
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

    let payload = EncryptionKeysEventContent {
        room_id: room.room_id().to_string(),
        member: CallMemberInfo {
            claimed_device_id: own_device.device_id.to_string(),
            id: call_id.to_string(),
        },
        keys: EncryptionKeysInfo {
            index,
            key: key.to_string(),
        },
        session: CallSessionInfo::default(),
        sent_ts: MilliSecondsSinceUnixEpoch::now(),
    };

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

/// Applies the call encryption keys from a sync batch's to-device events.
///
/// The keys arrive as per-device encrypted `io.element.call.encryption_keys`
/// events
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
            .and_then(|content| serde_json::from_value::<EncryptionKeysEventContent>(content).ok())
        else {
            event!(
                Level::WARN,
                "Received an undecodable call encryption key event"
            );
            continue;
        };

        apply_remote_key(&sender, &content, &livekit_room_manager).await;
    }
}

/// Rolls our call encryption key and distributes the new one whenever the
/// call's members change and we are in the call of an encrypted room.
///
/// Late joiners have missed the key we sent when joining, so this is the
/// only way they ever get to hear us.
pub async fn handle_call_member_change(
    _ev: OriginalSyncStateEvent<CallMemberEventContent>,
    room: matrix_sdk::Room,
    state: Ctx<AppState>,
) {
    if !room.encryption_state().is_encrypted() {
        return;
    }

    let livekit_room_manager = state.livekit_room_manager();

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
        // No connected call on this device. Our MatrixRTC membership may
        // still be flagged in the room, a leftover of a call that ended
        // without cleanup or a failed join,  and withdrawing it is the
        // only way the user can stop appearing in the call.
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
