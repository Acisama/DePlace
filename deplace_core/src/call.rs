//! Signaling between the call thread and the frontend.
//!
//! [`LivekitCall`] is the handle handed to the frontend when a call is
//! joined. Communication follows the split the rest of the app uses:
//!
//! - Rendering state lives in a [`Shared<CallState>`]:
//!   Widgets read it and hash it for lazy widget invalidation
//!   Its wake channel (`state_watch`) tells the
//!   message loop when it changed; it never carries data itself.
//! - Happenings and commands live in [`CallEvent`] and the methods on
//!   [`LivekitCall`]: They are
//!   delivered in order through a broadcast channel.
//!
//! The call thread folds its LiveKit events into the state on its own
//! task (see `matrix_api::matrixrtc`); the frontend task only ever reads
//! state and sends commands.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::SystemTime;

use dashmap::DashMap;
use indexmap::IndexMap;
use livekit::RoomEvent;
use livekit::prelude::{
    ConnectionState, Participant, ParticipantIdentity, RemoteTrackPublication, TrackSource,
};
use livekit::track::LocalAudioTrack;
use matrix_sdk::Room;
use rodio::buffer::SamplesBuffer;
use ruma::{OwnedDeviceId, OwnedRoomId, OwnedUserId, RoomId, UserId};
use tokio::sync::{broadcast, watch};

use crate::Shared;
use crate::audio::AudioThread;
use crate::colors::DePlaceColor;
use crate::matrix_api::matrixrtc::{LiveKitRoomManager, leave_matrixrtc_call};
use crate::member::ProfileStore;
use crate::member::synth::SignatureEvent;
use crate::room::call::{CallMember, CallMembers};
use crate::state::UserDevice;
use crate::theme::Theme;

/// A call participant, identified by their Matrix identity.
///
/// MatrixRTC LiveKit identities are `"{user_id}:{device_id}"`, anything
/// that doesn't parse as one keeps the
/// raw identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CallParticipantId {
    Member {
        user_id: OwnedUserId,
        device_id: OwnedDeviceId,
    },
    Other {
        identity: Arc<str>,
    },
}

impl CallParticipantId {
    pub(crate) fn new(user_id: OwnedUserId, device_id: OwnedDeviceId) -> Self {
        CallParticipantId::Member { user_id, device_id }
    }

    pub(crate) fn from_identity(identity: &ParticipantIdentity) -> Self {
        identity
            .as_str()
            .rsplit_once(':')
            .and_then(|(user_id, device_id)| {
                let user_id = ruma::UserId::parse(user_id).ok()?.to_owned();
                // Device ids aren't validated (any string is legal), so
                // there's no parse just a conversion.
                let device_id: OwnedDeviceId = device_id.into();
                Some(CallParticipantId::Member { user_id, device_id })
            })
            .unwrap_or(CallParticipantId::Other {
                identity: identity.as_str().into(),
            })
    }
}

/// A cell for storing the volume of a call participant.
#[derive(Default)]
pub struct VolumeCell(AtomicU32);

impl std::fmt::Debug for VolumeCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VolumeCell({})", self.get())
    }
}

impl VolumeCell {
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    /// A cell starting out at `v` instead of silence.
    pub fn with_value(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct CallParticipantHandle {
    pub(crate) participant: Arc<Shared<CallParticipant>>,
    /// How loud the participant is, pushed into by the call thread
    /// and read in the ui.
    pub(crate) volume: Arc<VolumeCell>,
    /// How loud this participant plays for us,
    /// the audio pump reads it every frame instead, UI writes it
    /// whenever the user changes it.
    pub(crate) playback_volume: Arc<VolumeCell>,
    /// Whether we muted them ourselves, again set by the ui
    /// and read by the call thread and applied to frames
    pub(crate) playback_muted: Arc<AtomicBool>,
}

impl Default for CallParticipantHandle {
    fn default() -> Self {
        Self {
            participant: Arc::new(Shared::new(CallParticipant::default())),
            volume: Arc::new(VolumeCell::default()),
            playback_volume: Arc::new(VolumeCell::with_value(1.0)),
            playback_muted: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl CallParticipantHandle {
    /// This participant's playback volume as a direct amplitude
    /// multiplier on their samples: `0.0` is silent, `1.0` is normal,
    /// `10.0` is ten times the amplitude. Loudness is perceived on a log
    /// scale (roughly +10dB, a ~3.16x amplitude jump, per perceived
    /// doubling), so the UI curves its slider percentage before it lands
    /// here; this value itself stays a plain linear multiplier since
    /// that's what the audio pump needs to scale samples with.
    /// Muting them doesn't touch this, so unmuting can return to it.
    pub fn playback_volume(&self) -> f32 {
        self.playback_volume.get()
    }

    /// Sets how loud this participant plays for us, clamped to
    /// `0.0..=10.0`.
    pub fn set_playback_volume(&self, volume: f32) {
        self.playback_volume.set(volume.clamp(0.0, 10.0));
    }

    /// Whether we muted this guy ourselves.
    pub fn playback_muted(&self) -> bool {
        self.playback_muted.load(Ordering::Relaxed)
    }

    /// Mute this fella or lifts our silencing again. Their
    /// playback volume is untouched, unmuting returns to whatever they
    /// played at before muting.
    pub fn set_playback_muted(&self, muted: bool) {
        self.playback_muted.store(muted, Ordering::Relaxed);
    }

    /// How loud they effectively play for us, their playback volume,
    /// or silence while we muted them.
    pub fn output_volume(&self) -> f32 {
        if self.playback_muted() {
            0.0
        } else {
            self.playback_volume()
        }
    }
}

/// State of any other call participant
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CallParticipant {
    pub speaking: bool,
    pub muted: bool,
    /// Whether they share their screen right now
    pub screensharing: bool,
    pub encryption: CallEncryptionState,
}

impl Default for CallParticipant {
    fn default() -> Self {
        Self {
            speaking: false,
            muted: false,
            screensharing: false,
            encryption: CallEncryptionState::NotConnected,
        }
    }
}

/// The E2EE state of a call participant, as reported by LiveKit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CallEncryptionState {
    // Not on LiveKit: Matrix-only member, left, or we're not in the call
    #[default]
    NotConnected,
    /// Connected, but LiveKit hasn't reported an E2EE state yet    Ok,
    Unknown,
    Ok,
    MissingKey,
    EncryptionFailed,
    DecryptionFailed,
    InternalError,
}

/// How far joining a call got, for showing progress on slow joins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallJoinStep {
    /// Fetching an OpenID token and exchanging it for a LiveKit one
    /// at the SFU
    Authenticating,
    /// Writing the call membership state event and ringing the room
    Signaling,
    /// Opening the LiveKit connection to the SFU
    Connecting,
    /// Setting up encryption keys and publishing the microphone
    Preparing,
}

impl CallJoinStep {
    pub fn label(&self) -> &'static str {
        match self {
            CallJoinStep::Authenticating => "Authenticating",
            CallJoinStep::Signaling => "Signaling",
            CallJoinStep::Connecting => "Connecting",
            CallJoinStep::Preparing => "Preparing",
        }
    }
}

pub(crate) type CallHandleMap = DashMap<CallParticipantId, CallParticipantHandle>;

/// Everything call-related on a room: always present.
#[derive(Debug)]
pub struct RoomCall {
    pub(crate) call_handles: Arc<CallHandleMap>,
    pub(crate) members: Shared<CallMembers>,
    own_device: Arc<UserDevice>,
    own_handle: CallParticipantHandle,

    pub(crate) own_volume: Arc<VolumeCell>,

    /// How far our own join into this room's call got, as long as it is
    /// still joining. `None` once the call is live or was given up, so
    /// a stuck step can be read off this on slow joins.
    pub(crate) join_progress: Shared<Option<CallJoinStep>>,

    /// The call we've joined in this room, if any.
    /// LiveKitCall doesn't implement `Clone`, so we use an `Arc` to share it.
    pub(crate) active: Shared<Option<Arc<LivekitCall>>>,
}

impl RoomCall {
    pub(crate) fn new(own_device: Arc<UserDevice>, own_handle: CallParticipantHandle) -> Self {
        Self {
            call_handles: Arc::new(CallHandleMap::new()),
            members: Shared::new(CallMembers::default()),
            own_device,
            own_handle,
            own_volume: Arc::new(VolumeCell::default()),
            join_progress: Shared::new(None),
            active: Shared::new(None),
        }
    }

    /// How far our own join into this room's call got, if we are joining
    /// one right now. Nothing once the call is live or was given up.
    pub fn join_progress(&self) -> Option<CallJoinStep> {
        *self.join_progress.get()
    }

    /// Returns the call handle for the given participant ID, creating it if necessary.
    pub(crate) fn call_handle(
        &self,
        user: OwnedUserId,
        device: OwnedDeviceId,
    ) -> CallParticipantHandle {
        let id = CallParticipantId::new(user.clone(), device.clone());
        self.call_handles
            .entry(id.clone())
            .or_insert_with(|| self.new_handle(&id))
            .clone()
    }

    pub(crate) fn new_handle(&self, id: &CallParticipantId) -> CallParticipantHandle {
        if let CallParticipantId::Member { user_id, device_id } = id
            && &self.own_device.user_id == user_id
            && &self.own_device.device_id == device_id
        {
            CallParticipantHandle {
                participant: self.own_handle.participant.clone(),
                volume: self.own_volume.clone(),
                playback_muted: Arc::new(AtomicBool::new(false)),
                playback_volume: Arc::new(VolumeCell::with_value(1.0)),
            }
        } else {
            CallParticipantHandle::default()
        }
    }

    pub fn own_user_is_in_call(&self) -> bool {
        self.members.read().own_user_is_in_call()
    }

    pub fn own_user_device_is_in_call(&self) -> bool {
        self.members.read().own_user_device_is_in_call()
    }

    pub fn call_participants(&self) -> Vec<CallMember> {
        self.members.read().members.clone()
    }

    pub fn call_participants_count(&self) -> usize {
        self.members.read().members.len()
    }

    pub fn users_in_call(&self) -> usize {
        self.members
            .read()
            .members
            .iter()
            .map(|m| m.member.user_id())
            .collect::<HashSet<_>>()
            .len()
    }

    pub fn set_own_mic_muted(&self, muted: bool) {
        if let Some(active) = self.active.read().as_ref() {
            active.set_microphone_muted(muted);
        }
    }
}

impl From<livekit::webrtc::native::frame_cryptor::EncryptionState> for CallEncryptionState {
    fn from(state: livekit::webrtc::native::frame_cryptor::EncryptionState) -> Self {
        use livekit::webrtc::native::frame_cryptor::EncryptionState;

        match state {
            EncryptionState::New => Self::Unknown,
            EncryptionState::Ok | EncryptionState::KeyRatcheted => Self::Ok,
            EncryptionState::MissingKey => Self::MissingKey,
            EncryptionState::EncryptionFailed => Self::EncryptionFailed,
            EncryptionState::DecryptionFailed => Self::DecryptionFailed,
            EncryptionState::InternalError => Self::InternalError,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CallConnectionState {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
    Ended,
}

impl CallConnectionState {
    pub fn icon(&self) -> &'static str {
        match self {
            CallConnectionState::Connecting => phosphor_svgs::icon::cell_signal_medium::BOLD,
            CallConnectionState::Connected => phosphor_svgs::icon::cell_signal_full::BOLD,
            CallConnectionState::Reconnecting => phosphor_svgs::icon::cell_signal_medium::BOLD,
            CallConnectionState::Ended => phosphor_svgs::icon::cell_signal_x::BOLD,
        }
    }

    pub fn color(&self, theme: Theme) -> DePlaceColor {
        match self {
            CallConnectionState::Connecting => theme.colors.warning,
            CallConnectionState::Connected => theme.colors.success,
            CallConnectionState::Reconnecting => theme.colors.warning,
            CallConnectionState::Ended => theme.colors.error,
        }
    }

    pub fn tooltip(&self) -> &'static str {
        match self {
            CallConnectionState::Connecting => "Connecting",
            CallConnectionState::Connected => "Connected",
            CallConnectionState::Reconnecting => "Reconnecting",
            CallConnectionState::Ended => "Ended",
        }
    }
}

/// Volume at which someone counts as speaking.
pub const SPEAKING_ON_VOLUME: f32 = 0.01;
pub const SPEAKING_OFF_VOLUME: f32 = 0.003;

/// Call events,  broadcast to whoever subscribed. Rendering
/// should always read
/// [`LivekitCall`] instead, which can't be missed.
#[derive(Clone, Debug)]
pub enum CallEvent {
    /// Our own connection was accepted by the SFU.
    Connected,
    ParticipantJoined {
        participant: CallParticipantId,
    },
    ParticipantLeft {
        participant: CallParticipantId,
    },
    EncryptionStateChanged {
        participant: CallParticipantId,
        state: CallEncryptionState,
    },
    /// The call is over: [`CallEndReason::Left`] after
    /// `DePlaceCall::leave`, [`CallEndReason::Disconnected`] when the
    /// connection dropped on its own.
    Ended {
        reason: CallEndReason,
    },
    CommandFailed {
        command: CallCommand,
        error: Arc<str>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallEndReason {
    Left,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Interactions with the call from the UI
///
/// Will be expanded
pub enum CallCommand {
    Leave,
    MuteMicrophone,
    UnmuteMicrophone,
}

/// The frontend-facing handle of one ongoing call.
///
/// Clones share the same call; it stays valid until the call ends, after
/// which `state.connection` is [`CallConnectionState::Ended`].
pub struct LivekitCall {
    room: Room,
    manager: LiveKitRoomManager,
    audio: AudioThread,
    own_device: Arc<UserDevice>,

    profile_store: Arc<ProfileStore>,

    own_handle: CallParticipantHandle,

    own_volume: Arc<VolumeCell>,
    started_at: SystemTime,

    own_connection: Shared<CallConnectionState>,
    /// Bumped when `state` actually changed, the wake-up signal for the
    /// frontend's message loop. Carries no data
    state_wake: watch::Sender<()>,
    /// One-shot call happenings
    events: broadcast::Sender<CallEvent>,
    /// The published microphone track, to mute and unmute at will.
    microphone: Shared<Option<LocalAudioTrack>>,

    call_handles: Arc<CallHandleMap>,
}

impl std::fmt::Debug for LivekitCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DePlaceCall")
            .field("room_id", &self.room.room_id())
            .finish_non_exhaustive()
    }
}

impl PartialEq for LivekitCall {
    fn eq(&self, other: &Self) -> bool {
        self.room.room_id() == other.room.room_id()
    }
}

impl Eq for LivekitCall {}

// Deliberately hashes more than `PartialEq` compares: the hash is for
// widget invalidation (it changes with the state), so don't use a
// `LiveKitCall` as a `HashMap`/`HashSet` key.
impl Hash for LivekitCall {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.room.room_id().hash(state);
        self.own_connection.hash(state);
    }
}

/// Plays an already-rendered signature sound through some [`AudioThread`],
/// fire-and-forget. `event` is only used for logging if playback fails.
pub(crate) fn play_signature_samples(
    audio: &AudioThread,
    samples: SamplesBuffer<f32>,
    event: SignatureEvent,
) {
    let audio = audio.clone();

    tokio::spawn(async move {
        match audio.play_source(samples).await {
            Ok(sound) => sound.detach(),
            Err(e) => tracing::warn!("Could not play {event:?} sound: {e:#}"),
        }
    });
}

impl LivekitCall {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        room: Room,
        manager: LiveKitRoomManager,
        audio: AudioThread,
        call_handles: Arc<CallHandleMap>,
        own_device: Arc<UserDevice>,
        own_volume: Arc<VolumeCell>,
        state_wake: watch::Sender<()>,
        own_handle: CallParticipantHandle,
        profile_store: Arc<ProfileStore>,
    ) -> Self {
        let (events, _) = broadcast::channel(64);

        Self {
            started_at: SystemTime::now(),
            room,
            audio,
            own_device,
            own_volume,
            manager,
            own_connection: Shared::new(CallConnectionState::Connecting),
            own_handle,
            state_wake,
            events,
            microphone: Shared::new(None),
            call_handles,
            profile_store,
        }
    }

    pub fn room_id(&self) -> &RoomId {
        self.room.room_id()
    }

    /// Returns the call handle for the given participant, creating it if
    /// necessary. The room side creates the same handles from the Matrix
    /// memberships, so the miss path has to go through `entry` to never
    /// overwrite a handle someone else just inserted.
    pub(crate) fn call_handle(&self, id: &CallParticipantId) -> CallParticipantHandle {
        // Fast path without cloning the key. The read guard is dropped
        // before `entry` takes the write lock.
        if let Some(handle) = self.call_handles.get(id) {
            return handle.clone();
        }
        self.call_handles
            .entry(id.clone())
            .or_insert_with(|| self.new_handle(id))
            .clone()
    }

    pub(crate) fn new_handle(&self, id: &CallParticipantId) -> CallParticipantHandle {
        if let CallParticipantId::Member { user_id, device_id } = id
            && &self.own_device.user_id == user_id
            && &self.own_device.device_id == device_id
        {
            CallParticipantHandle {
                participant: self.own_handle.participant.clone(),
                volume: self.own_volume.clone(),
                playback_muted: Arc::new(AtomicBool::new(false)),
                playback_volume: Arc::new(VolumeCell::with_value(1.0)),
            }
        } else {
            CallParticipantHandle::default()
        }
    }

    /// Changes a participant's state, waking the frontend only when it
    /// actually changed. The closure may be retried by the underlying
    /// compare-and-swap.
    pub(crate) fn change_participant(
        &self,
        handle: &CallParticipantHandle,
        change: impl Fn(&mut CallParticipant),
    ) {
        let before = handle.participant.version();
        handle.participant.change_if_modified(change);

        if handle.participant.version() != before {
            self.wake();
        }
    }

    /// Resets everyone to "not connected", for when we're no longer in
    /// the call ourselves. The handles stay in the map, the room's
    /// `CallMembers` still point at them.
    pub(crate) fn reset_participants(&self) {
        for entry in self.call_handles.iter() {
            let handle = entry.value();
            handle.volume.set(0.0);
            self.change_participant(handle, |p| *p = CallParticipant::default());
        }
    }

    pub(crate) fn wake(&self) {
        self.state_wake.send_modify(|_| ());
    }

    /// Plays an already-rendered signature sound, fire-and-forget. `event`
    /// is only used for logging if playback fails.
    pub(crate) fn play_samples(&self, samples: SamplesBuffer<f32>, event: SignatureEvent) {
        play_signature_samples(&self.audio, samples, event);
    }

    /// Plays a participant's signature sound for one of the call's
    /// happenings
    fn play_signature_sound(&self, room_id: &RoomId, user_id: &UserId, event: SignatureEvent) {
        let samples = self
            .profile_store
            .get_member(room_id, user_id)
            .calc_synth_sound(event);
        self.play_samples(samples, event);
    }

    /// Plays a signature sound for the local participant, if they are a member.
    pub(crate) fn play_own_signature_sound(&self, room_id: &RoomId, event: SignatureEvent) {
        self.play_signature_sound(room_id, &self.own_device.user_id, event);
    }

    /// Plays a signature sound for a participant, if they are a member.
    pub(crate) fn maybe_play_signature_sound(
        &self,
        room_id: &RoomId,
        id: &CallParticipantId,
        event: SignatureEvent,
    ) {
        if let CallParticipantId::Member { user_id, .. } = id {
            self.play_signature_sound(room_id, user_id, event);
        }
    }

    /// Wake-up signal for the frontend's message loop: fires whenever the
    /// state actually changed.
    pub fn state_watch(&self) -> watch::Receiver<()> {
        self.state_wake.subscribe()
    }

    /// Ordered one-shot call happenings, for whoever needs to react to
    /// more than the rendered state.
    pub fn subscribe(&self) -> broadcast::Receiver<CallEvent> {
        self.events.subscribe()
    }

    /// Mute or unmute our own microphone, if one was published.
    pub fn set_microphone_muted(&self, muted: bool) {
        let command = if muted {
            CallCommand::MuteMicrophone
        } else {
            CallCommand::UnmuteMicrophone
        };

        let microphone = self.microphone.get();
        let Some(microphone) = microphone.as_ref() else {
            let error: Arc<str> = "no microphone was published".into();
            tracing::warn!(
                "Can't {} the call in {}: {}",
                if muted { "mute" } else { "unmute" },
                self.room.room_id(),
                error
            );
            self.emit(CallEvent::CommandFailed { command, error });
            return;
        };

        if muted {
            microphone.mute();
        } else {
            microphone.unmute();
        }

        self.own_handle
            .participant
            .change_if_modified(|p| p.muted = muted);
        self.change_own_participant(|p| p.muted = muted);
    }

    /// The mute state of our own microphone.
    pub fn is_microphone_muted(&self) -> bool {
        self.own_handle.participant.read().muted
    }

    /// Leave the call. Disconnects from the SFU, removes our MatrixRTC
    /// call member state, and ends up emitting
    /// `CallEvent::Ended { Left }` from the call thread.
    pub async fn leave(&self) {
        if let Err(e) =
            leave_matrixrtc_call(&self.room, &self.own_device, self.manager.clone()).await
        {
            tracing::error!("Failed to leave the call in {}: {e:#}", self.room.room_id());
            self.emit(CallEvent::CommandFailed {
                command: CallCommand::Leave,
                error: e.to_string().into(),
            });
        }
    }

    // the following should only be used by the  Call thread

    /// Reports how loud a remote participant currently is, measured from
    /// the audio we receive from them. Called at frame cadence by the
    /// track pump
    pub(crate) fn set_participant_volume(&self, handle: &CallParticipantHandle, volume: f32) {
        handle.volume.set(volume);
        self.change_participant(handle, |p| p.speaking = speaking_from(p.speaking, volume));
    }

    /// Reports how loud our own microphone currently is, the mic pump
    /// already gates this to `0.0` while muted.
    pub(crate) fn set_own_volume(&self, volume: f32) {
        self.own_volume.set(volume);

        self.change_own_participant(|p| {
            p.speaking = speaking_from(p.speaking, volume);
        });
    }

    /// Forgets a remote participant's volume, if their track went away or
    /// they left.
    pub(crate) fn clear_participant_volume(&self, handle: &CallParticipantHandle) {
        handle.volume.set(0.0);
        self.change_participant(handle, |p| p.speaking = false);
    }

    /// Registers the published microphone track so commands can find it.
    pub(crate) fn set_microphone(&self, track: LocalAudioTrack) {
        self.microphone.set(Some(track));
    }

    /// Applies a change to the call state, waking the frontend only when
    /// the state actually changed. The closure
    /// may be retried by the underlying compare-and-swap.
    pub(crate) fn change_own_participant(&self, change: impl Fn(&mut CallParticipant)) {
        let before = self.own_handle.participant.version();
        self.own_handle.participant.change_if_modified(change);

        if self.own_handle.participant.version() != before {
            self.wake();
        }
    }

    pub(crate) fn change_own_connection(&self, connection: CallConnectionState) {
        self.own_connection.change_if_modified(|p| *p = connection);
    }

    pub(crate) fn own_connection(&self) -> Arc<CallConnectionState> {
        self.own_connection.get()
    }

    pub(crate) fn emit(&self, event: CallEvent) {
        if self.events.send(event).is_err() {
            tracing::trace!("Dropping a call event with no subscriber subscribed");
        }
    }

    /// Folds one LiveKit room event into the call state, emitting the
    /// one-shot happenings alongside.
    pub(crate) fn handle_room_event(&self, event: &RoomEvent) {
        let room_id = self.room_id();
        match event {
            RoomEvent::Connected {
                participants_with_tracks,
            } => {
                // Outside of `change_state`: its closure may be retried
                for (participant, publications) in participants_with_tracks {
                    let id = CallParticipantId::from_identity(&participant.identity());
                    let joined = participant_from_publications(publications);
                    self.change_participant(&self.call_handle(&id), |p| *p = joined.clone());
                }
                self.change_own_connection(CallConnectionState::Connected);
                self.emit(CallEvent::Connected);
            }
            RoomEvent::ParticipantConnected(participant) => {
                let id = CallParticipantId::from_identity(&participant.identity());

                // The handle usually exists already (created from the Matrix
                // membership, or kept from an earlier session), so set the
                // fresh state instead of relying on an initializer.
                // A brand-new participant can't be heard yet.
                let handle = self.call_handle(&id);
                handle.volume.set(0.0);
                self.change_participant(&handle, |p| {
                    *p = CallParticipant {
                        speaking: false,
                        muted: true,
                        screensharing: false,
                        encryption: CallEncryptionState::Unknown,
                    }
                });

                // Joined/left sounds are driven by the `CallMembers` diff
                // instead (see `matrix_api::sync`), so they fire in step
                // with what the UI's participant list actually shows.
                self.emit(CallEvent::ParticipantJoined { participant: id });
            }
            RoomEvent::ParticipantDisconnected(participant) => {
                let id = CallParticipantId::from_identity(&participant.identity());

                // Reset, never remove: `CallMembers` may still hold the
                // handle, and a reconnect should find the same one again.
                // Without a handle there's nothing to reset.
                let handle = self.call_handle(&id);
                handle.volume.set(0.0);
                self.change_participant(&handle, |p| *p = CallParticipant::default());

                self.emit(CallEvent::ParticipantLeft { participant: id });
            }
            // ActiveSpeakersChanged is deliberately *not* handled: the
            // active-speaker indication is unreliable. Speaking and
            // loudness are measured ourselves from the received audio
            // (see `set_participant_volume`).
            RoomEvent::TrackMuted {
                participant,
                publication,
            } if publication.source() == TrackSource::Microphone => match participant {
                Participant::Remote(remote) => {
                    let id = CallParticipantId::from_identity(&remote.identity());
                    self.change_participant(&self.call_handle(&id), |p| p.muted = true);
                    self.maybe_play_signature_sound(room_id, &id, SignatureEvent::Muted);
                }
                Participant::Local(_) => {
                    self.change_own_participant(|p| p.muted = true);
                    self.play_own_signature_sound(room_id, SignatureEvent::Muted);
                }
            },
            RoomEvent::TrackUnmuted {
                participant,
                publication,
            } if publication.source() == TrackSource::Microphone => match participant {
                Participant::Remote(remote) => {
                    let id = CallParticipantId::from_identity(&remote.identity());
                    self.change_participant(&self.call_handle(&id), |p| p.muted = false);
                    self.maybe_play_signature_sound(room_id, &id, SignatureEvent::Unmuted);
                }
                Participant::Local(_) => {
                    self.change_own_participant(|p| p.muted = false);
                    self.play_own_signature_sound(room_id, SignatureEvent::Unmuted);
                }
            },
            RoomEvent::TrackPublished {
                participant,
                publication,
            } if publication.source() == TrackSource::Microphone => {
                let id = CallParticipantId::from_identity(&participant.identity());
                let muted = publication.is_muted();
                self.change_participant(&self.call_handle(&id), |p| p.muted = muted);
                self.maybe_play_signature_sound(
                    room_id,
                    &id,
                    if muted {
                        SignatureEvent::Muted
                    } else {
                        SignatureEvent::Unmuted
                    },
                );
            }
            RoomEvent::TrackPublished {
                participant,
                publication,
            } if publication.source() == TrackSource::Screenshare => {
                let id = CallParticipantId::from_identity(&participant.identity());
                self.change_participant(&self.call_handle(&id), |p| p.screensharing = true);
                self.maybe_play_signature_sound(room_id, &id, SignatureEvent::StartedScreenShare);
            }
            RoomEvent::TrackUnpublished {
                participant,
                publication,
            } if publication.source() == TrackSource::Microphone => {
                let id = CallParticipantId::from_identity(&participant.identity());
                self.change_participant(&self.call_handle(&id), |p| p.muted = true);
                self.maybe_play_signature_sound(room_id, &id, SignatureEvent::Muted);
            }
            RoomEvent::TrackUnpublished {
                participant,
                publication,
            } if publication.source() == TrackSource::Screenshare => {
                let id = CallParticipantId::from_identity(&participant.identity());
                self.change_participant(&self.call_handle(&id), |p| p.screensharing = false);
                self.maybe_play_signature_sound(room_id, &id, SignatureEvent::StoppedScreenShare);
            }
            RoomEvent::E2eeStateChanged {
                participant,
                state: encryption,
            } => {
                let encryption = CallEncryptionState::from(*encryption);
                if encryption != CallEncryptionState::Ok
                    && encryption != CallEncryptionState::Unknown
                {
                    tracing::warn!("Call encryption problems with {participant:?}: {encryption:?}");
                }
                let id = CallParticipantId::from_identity(&participant.identity());
                self.change_participant(&self.call_handle(&id), |p| p.encryption = encryption);
                self.emit(CallEvent::EncryptionStateChanged {
                    participant: id,
                    state: encryption,
                });
            }
            RoomEvent::ConnectionStateChanged(connection) => {
                let connection = match connection {
                    ConnectionState::Connected => CallConnectionState::Connected,
                    ConnectionState::Reconnecting => CallConnectionState::Reconnecting,
                    ConnectionState::Disconnected => CallConnectionState::Ended,
                };
                self.change_own_connection(connection);
            }
            _ => {}
        }
    }

    pub(crate) fn started_at(&self) -> SystemTime {
        self.started_at
    }
}

/// For the speaking flag: flips on above [`SPEAKING_ON_VOLUME`],
/// stays on until the volume drops below [`SPEAKING_OFF_VOLUME`].
fn speaking_from(currently_speaking: bool, volume: f32) -> bool {
    if currently_speaking {
        volume >= SPEAKING_OFF_VOLUME
    } else {
        volume >= SPEAKING_ON_VOLUME
    }
}

/// What a joined participant's microphone looks like based on the
/// publications they arrived with, no microphone at all counts as muted.
fn participant_from_publications(publications: &[RemoteTrackPublication]) -> CallParticipant {
    let mut microphones = publications
        .iter()
        .filter(|publication| publication.source() == TrackSource::Microphone);

    CallParticipant {
        speaking: false,
        muted: microphones.next().is_none_or(|p| p.is_muted()),
        screensharing: publications
            .iter()
            .any(|publication| publication.source() == TrackSource::Screenshare),
        encryption: CallEncryptionState::Unknown,
    }
}

/// All ongoing calls, keyed by room. Shared so views can read and hash
/// it, with a wake channel that fires on joins and leaves, everything
/// within a call arrives through the call's own channels instead.
#[derive(Clone)]
pub struct CallRegistry {
    calls: Arc<Shared<IndexMap<OwnedRoomId, Arc<LivekitCall>>>>,
    watch: watch::Sender<u64>,
}

impl std::fmt::Debug for CallRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.calls.get().keys()).finish()
    }
}

impl Default for CallRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CallRegistry {
    pub fn new() -> Self {
        let (watch, _) = watch::channel(0);

        Self {
            calls: Arc::new(Shared::new(IndexMap::new())),
            watch,
        }
    }

    /// The ongoing call in a room, if there is one.
    pub fn get(&self, room_id: &RoomId) -> Option<Arc<LivekitCall>> {
        self.calls.get().get(room_id).cloned()
    }

    /// Leaves every ongoing call, e.g. when the app is about to close
    /// so we don't ghost out
    pub async fn leave_all(&self) {
        for (_, call) in self.all().iter() {
            call.leave().await;
        }
    }

    /// A snapshot of all ongoing calls, keyed by room.
    pub fn all(&self) -> Arc<IndexMap<OwnedRoomId, Arc<LivekitCall>>> {
        self.calls.get()
    }

    /// Wake-up signal for the frontend's message loop: fires when a call
    /// was joined or left.
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.watch.subscribe()
    }

    pub(crate) fn insert(&self, call: Arc<LivekitCall>) {
        let room_id = call.room_id().to_owned();
        self.calls.change_if_modified(|calls| {
            calls.insert(room_id.clone(), call.clone());
        });
        self.watch.send_modify(|tick| *tick = tick.wrapping_add(1));
    }

    pub(crate) fn remove(&self, room_id: &RoomId) {
        if self.calls.get().contains_key(room_id) {
            self.calls.change_if_modified(|calls| {
                calls.shift_remove(room_id);
            });
            self.watch.send_modify(|tick| *tick = tick.wrapping_add(1));
        }
    }
}
