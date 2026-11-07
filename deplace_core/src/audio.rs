//! Everything that makes noise lives here.
//!
//! This module is the only thing that talks to the sound device, through
//! cpal and rodio. All sounds are mixed into one output stream.
//!
//! Sounds play once via one of the play methods, call audio arrives frame
//! by frame through a frame player and microphones are recorded through
//! a microphone handle.
//!
//! Audio may not cross threads, so it lives on its own thread and is
//! reached through AudioThread, which forwards every request.
//!
//! Whatever happens to the sound devices, Audio never has to be built
//! again and the app never restarts. It works like this.
//!
//! Nothing is connected right away, the output device opens on the
//! first sound and reopens on the next one whenever it was lost. On the
//! idle polling between requests the thread watches for the system
//! default device changing and plugs or unplugs being yanked.
//!
//! Switching devices just rebuilds the streams inside Audio. Frame
//! players keep their sample buffers and microphones keep their handles,
//! both simply get rewired to the new device, so live calls survive a
//! switch with a blip instead of a dead call. Only detached one-shot
//! sounds stop forever, everything else carries on.
//!
//! Devices can also be chosen, through set_output_device and
//! set_input_device. The choice pins all future streams to that device,
//! the connection for it is opened right away and every later sound and
//! microphone uses it. Handing None over lets Audio follow the system
//! default again. A chosen device that vanished falls back to the
//! default until it returns.

use std::{
    collections::{HashMap, VecDeque},
    io::{BufReader, Cursor},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::RecvTimeoutError,
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use livekit::webrtc::prelude::AudioFrame;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use tokio::sync::oneshot;
use tracing::{Level, event};

/// The sample rate LiveKit sends and the sound device wants
const OUTPUT_SAMPLE_RATE: u32 = 48_000;
/// Everything is mixed down to two channels
const OUTPUT_CHANNELS: u16 = 2;
/// How much audio may pile up before the oldest samples are dropped
const MAX_BUFFERED_SAMPLES: usize = 12_000;
/// How often the audio thread checks for device changes
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Name of the current default output device, if there is one.
fn default_output_name() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .and_then(|device| device.name().ok())
}

/// Name of the current default input device, if there is one.
fn default_input_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|device| device.name().ok())
}

/// The currently open output device, plus the handle everything plays
/// through combined into a connection which is rebuilt whenever the device
/// is switched or lost, so nothing
/// else may hold on to it.
struct OutputConn {
    _stream: OutputStream,
    mixer: OutputStreamHandle,
}

/// Connection to the sound device.
///
/// Create one `Audio` when the app starts and keep it alive until the app
/// closes. The output device is opened lazily and can be swapped
///  at any time, so device changes never require restarting
/// the app or recreating `Audio`.
pub struct Audio {
    /// The currently open output device, can be `None`, sound
    /// requests then try to open one before failing.
    output: Option<OutputConn>,
    /// The output device to use, or `None` to follow the system default.
    output_device: Option<cpal::Device>,
    /// Name of the default output device last seen, to notice changes.
    last_default_output: Option<String>,
    /// Live frame players for call audio, so their sinks can be recreated
    /// after a device switch. Their sample buffers live on, so a switch
    /// is just a blip, not a dead call.
    frame_players: HashMap<u64, FramePlayerSlot>,
    /// Id the next frame player gets.
    next_player_id: u64,
    /// One capture stream per open microphone handle.
    mics: HashMap<u64, MicSlot>,
    /// The input device to use, or `None` to follow the system default.
    input_device: Option<cpal::Device>,
    /// Name of the default input device last seen, to notice changes.
    last_default_input: Option<String>,
    /// Id the next microphone gets.
    next_mic_id: u64,
}

/// One live frame player, as the audio thread sees it.
struct FramePlayerSlot {
    /// The sink pumping the samples into the current output device.
    /// Recreated on every device switch.
    sink: Sink,
    /// The buffer frames from LiveKit land in. Kept across switches.
    samples: Arc<SharedSamples>,
}

/// One microphone capture, as the audio thread sees it.
struct MicSlot {
    /// The capture stream, or None while it could not be opened.
    /// Polling keeps retrying to revive the microphone.
    stream: Option<cpal::Stream>,
    /// The buffer the microphone fills. Kept so the handle stays valid
    /// across device switches and retry gaps.
    samples: Arc<MicSamples>,
}

impl Audio {
    /// Does not connect yet, the output device opens on first use so the
    /// app can start with no sound device at all.
    fn new() -> Self {
        Self {
            output: None,
            output_device: None,
            // Prime the names so the first idle poll doesn't reopen a
            // perfectly fine device just because there was nothing to
            // compare against yet.
            last_default_output: default_output_name(),
            frame_players: HashMap::new(),
            next_player_id: 0,
            mics: HashMap::new(),
            input_device: None,
            last_default_input: default_input_name(),
            next_mic_id: 0,
        }
    }

    /// Opens the output device if none is open. Every sound-making
    /// method goes through this, so a lost device is reconnected on the
    /// very next sound instead of killing anything.
    fn open(&mut self) -> Result<()> {
        if self.output.is_none() {
            self.output = Some(self.build_output()?);
        }
        Ok(())
    }

    /// Opens a connection, honoring the device choice, with a fallback
    /// to the default if the chosen one has vanished.
    fn build_output(&self) -> Result<OutputConn> {
        let (stream, mixer) = match &self.output_device {
            Some(device) => OutputStream::try_from_device(device)
                .with_context(|| {
                    format!(
                        "Could not open the output device {}",
                        device.name().unwrap_or_else(|_| "unknown".into())
                    )
                })
                .or_else(|first_err| {
                    event!(
                        Level::WARN,
                        "The chosen output device is gone, trying the default: {first_err:#}"
                    );
                    OutputStream::try_default()
                        .context("Could not open the default sound output device")
                })?,
            None => OutputStream::try_default()
                .context("Could not open the default sound output device")?,
        };

        Ok(OutputConn {
            _stream: stream,
            mixer,
        })
    }

    /// Rebuilds the output connection. Live frame players keep their
    /// buffers and are rewired; detached one-shot sounds simply stop.
    fn rebuild_output(&mut self) {
        let conn = match self.build_output() {
            Ok(conn) => conn,
            Err(error) => {
                event!(
                    Level::WARN,
                    "No output device available, will retry: {error:#}"
                );
                self.output = None;
                return;
            }
        };

        let had_old = self.output.is_some();
        self.output = Some(conn);

        if had_old {
            event!(Level::INFO, "Switched the sound output device");
        }

        // Rewire live call players to the new device.
        let mixer = &self.output.as_ref().expect("just set").mixer;
        for slot in self.frame_players.values_mut() {
            match Sink::try_new(mixer) {
                Ok(sink) => {
                    sink.append(FrameSource {
                        samples: Arc::clone(&slot.samples),
                    });
                    slot.sink = sink;
                }
                Err(error) => event!(
                    Level::ERROR,
                    "Could not rewire a call player to the new device: {error}"
                ),
            }
        }
    }

    /// Changes the output device, or back to following the default.
    fn set_output_device(&mut self, device: Option<cpal::Device>) {
        self.output_device = device;
        self.rebuild_output();
    }

    /// Changes the input device, or back to following the default.
    /// Open microphones are rebuilt but keep their buffers and handles.
    fn set_input_device(&mut self, device: Option<cpal::Device>) {
        self.input_device = device;
        self.rebuild_mics();
    }

    /// Reopens every microphone capture stream, e.g. after the input
    /// device changed. Microphone handles keep working throughout.
    fn rebuild_mics(&mut self) {
        for slot in self.mics.values_mut() {
            let restored = || -> Result<cpal::Stream> {
                let device = match &self.input_device {
                    Some(device) => device.clone(),
                    None => cpal::default_host()
                        .default_input_device()
                        .context("There is no default microphone")?,
                };
                let config = device
                    .default_input_config()
                    .context("Could not read the microphone's format")?;
                let stream = build_mic_stream(&device, &config, Arc::clone(&slot.samples))?;
                stream.play().context("Could not start the microphone")?;
                Ok(stream)
            };

            match restored() {
                Ok(stream) => slot.stream = Some(stream),
                Err(error) => {
                    event!(
                        Level::WARN,
                        "Could not open the microphone, will retry: {error:#}"
                    );
                    slot.stream = None;
                }
            }
        }
    }

    /// Whether the default output device changed since the last check.
    fn default_output_changed(&mut self) -> bool {
        if self.output_device.is_some() {
            // The user pinned a device, don't follow the default.
            return false;
        }

        let name = default_output_name();

        match (&self.last_default_output, &name) {
            (Some(old), Some(new)) if old == new => false,
            _ => {
                self.last_default_output = name;
                true
            }
        }
    }

    /// Whether the default input device changed since the last check.
    fn default_input_changed(&mut self) -> bool {
        if self.input_device.is_some() {
            return false;
        }

        let name = default_input_name();

        match (&self.last_default_input, &name) {
            (Some(old), Some(new)) if old == new => false,
            _ => {
                self.last_default_input = name;
                true
            }
        }
    }

    /// Plays an audio file and returns a handle to it.
    pub fn play_file(&mut self, path: impl AsRef<Path>) -> Result<Sound> {
        self.open()?;
        let file = std::fs::File::open(path).context("Could not open the audio file")?;

        let sink = self
            .output
            .as_ref()
            .expect("just opened")
            .mixer
            .play_once(BufReader::new(file))
            .context("Could not decode/play the audio file")?;

        Ok(Sound { sink })
    }

    /// Plays audio data that is already in memory.
    pub fn play_data(&mut self, data: impl Into<Vec<u8>>) -> Result<Sound> {
        self.open()?;
        let sink = self
            .output
            .as_ref()
            .expect("just opened")
            .mixer
            .play_once(Cursor::new(data.into()))
            .context("Could not decode/play the audio data")?;

        Ok(Sound { sink })
    }

    /// Plays an already-decoded rodio source.
    pub fn play_source<S>(&mut self, source: S) -> Result<Sound>
    where
        S: Source<Item = f32> + Send + 'static,
    {
        self.open()?;
        let sink = Sink::try_new(&self.output.as_ref().expect("just opened").mixer)
            .context("Could not create a sink")?;
        sink.append(source);
        Ok(Sound { sink })
    }

    /// Creates a new player for live audio that arrives frame by frame
    /// Each player is mixed in with everything
    /// else and can be dropped to stop it.
    fn frame_player(&mut self, cmd_tx: &std::sync::mpsc::Sender<Command>) -> Result<FramePlayer> {
        self.open()?;
        let samples = Arc::new(SharedSamples::default());
        let sink = Sink::try_new(&self.output.as_ref().expect("just opened").mixer)
            .context("Could not create a frame player")?;
        sink.append(FrameSource {
            samples: Arc::clone(&samples),
        });

        let id = self.next_player_id;
        self.next_player_id += 1;
        self.frame_players.insert(
            id,
            FramePlayerSlot {
                sink,
                samples: Arc::clone(&samples),
            },
        );

        Ok(FramePlayer {
            id,
            samples,
            cmd_tx: cmd_tx.clone(),
        })
    }

    /// Starts recording the default or chosen microphone, drop the
    /// handle to stop it.
    fn microphone(&mut self, cmd_tx: &std::sync::mpsc::Sender<Command>) -> Result<Microphone> {
        let device = match &self.input_device {
            Some(device) => device.clone(),
            None => cpal::default_host()
                .default_input_device()
                .context("There is no default microphone")?,
        };

        let config = device
            .default_input_config()
            .context("Could not read the microphone's format")?;

        let samples = Arc::new(MicSamples::default());
        let stream = build_mic_stream(&device, &config, Arc::clone(&samples))?;
        stream.play().context("Could not start the microphone")?;

        let id = self.next_mic_id;
        self.next_mic_id += 1;
        self.mics.insert(
            id,
            MicSlot {
                stream: Some(stream),
                samples: Arc::clone(&samples),
            },
        );

        Ok(Microphone {
            id,
            samples,
            cmd_tx: cmd_tx.clone(),
        })
    }

    /// Whether live call players are waiting for a working device, so a
    /// lost output should be retried even without new requests coming in.
    fn frame_players_open(&self) -> bool {
        !self.frame_players.is_empty()
    }

    /// Stops the capture of a dropped microphone handle.
    fn stop_microphone(&mut self, id: u64) {
        self.mics.remove(&id);
    }

    /// Stops the frame player of a dropped handle.
    fn stop_frame_player(&mut self, id: u64) {
        self.frame_players.remove(&id);
    }
}

/// A sound that is playing right now. It keeps playing until the handle
/// is dropped or the sound ends.
pub struct Sound {
    sink: Sink,
}

impl Sound {
    /// Sets how loud this sound is. 0.0 is silent, 1.0 is normal, anything
    /// above boosts it.
    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    /// Pauses this sound until it is played again.
    pub fn pause(&self) {
        self.sink.pause();
    }

    /// Resumes a paused sound.
    pub fn play(&self) {
        self.sink.play();
    }

    /// Stops the sound immediately
    pub fn stop(self) {
        // Dropping the sink stops playback so we just drop self
        // I'm so proud of this
    }

    /// Lets the sound keep playing to the end even after this handle is
    /// dropped, for fire-and-forget playback where nothing holds onto the
    /// handle. Without this, dropping the handle stops the sound dead
    pub fn detach(self) {
        self.sink.detach();
    }
}

/// Audio that is usable from async code.
///
/// Audio may not cross threads, so the real one sits on its own thread
/// and this handle forwards requests to it through a channel. From the
/// outside it looks just like the real thing.
///
/// Keep one alive as long as you want to make sounds.
#[derive(Clone)]
pub struct AudioThread {
    /// The end of the channel the audio thread listens to. As long as
    /// this lives, the thread lives.
    cmd_tx: std::sync::mpsc::Sender<Command>,
}

/// The requests that can be sent to the little audio thread.
enum Command {
    FramePlayer {
        reply: oneshot::Sender<Result<FramePlayer>>,
    },
    PlayFile {
        path: PathBuf,
        reply: oneshot::Sender<Result<Sound>>,
    },
    PlayData {
        data: Vec<u8>,
        reply: oneshot::Sender<Result<Sound>>,
    },
    PlaySource {
        source: Box<dyn Source<Item = f32> + Send + Sync>,
        reply: oneshot::Sender<Result<Sound>>,
    },
    Microphone {
        reply: oneshot::Sender<Result<Microphone>>,
    },
    StopMicrophone {
        id: u64,
    },
    StopFramePlayer {
        id: u64,
    },
    /// `None` for the device means "follow the system default" again.
    SetOutputDevice {
        device: Option<cpal::Device>,
        reply: oneshot::Sender<Result<()>>,
    },
    SetInputDevice {
        device: Option<cpal::Device>,
        reply: oneshot::Sender<Result<()>>,
    },
}

impl AudioThread {
    /// Starts the audio thread. Do this once and keep the handle around,
    /// clone it wherever needed.
    pub fn start() -> Result<Self> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        // Just used to find out whether the connection worked, the thread
        // answers actual requests through the oneshots in the commands.
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        // Cloned into every Microphone so it can announce its own death.
        let stop_tx = cmd_tx.clone();

        std::thread::Builder::new()
            .name("deplace-audio".into())
            .spawn(move || {
                // No device is opened here: the audio thread always starts,
                // even on a machine with no sound device at all, and opens
                // (and reopens) the device lazily as sounds are requested.
                let mut audio = Audio::new();

                drop(init_tx.send(Ok(())));

                // Answer requests until everyone is gone (last handle
                // dropped). Between requests, poll for device changes so
                // plugging/unplugging headphones or switching the system
                // default is picked up without restarting anything.
                // Nobody listening to the answer anymore is fine, the
                // sound happens either way. Or does it? I'm going crazy over here
                loop {
                    match cmd_rx.recv_timeout(POLL_INTERVAL) {
                        Ok(command) => match command {
                            Command::FramePlayer { reply } => {
                                drop(reply.send(audio.frame_player(&stop_tx)));
                            }
                            Command::PlayFile { path, reply } => {
                                drop(reply.send(audio.play_file(path)));
                            }
                            Command::PlayData { data, reply } => {
                                drop(reply.send(audio.play_data(data)));
                            }
                            Command::PlaySource { source, reply } => {
                                drop(reply.send(audio.play_source(source)));
                            }
                            Command::Microphone { reply } => {
                                drop(reply.send(audio.microphone(&stop_tx)));
                            }
                            Command::StopMicrophone { id } => {
                                audio.stop_microphone(id);
                            }
                            Command::StopFramePlayer { id } => {
                                audio.stop_frame_player(id);
                            }
                            Command::SetOutputDevice { device, reply } => {
                                audio.set_output_device(device);
                                drop(reply.send(Ok(())));
                            }
                            Command::SetInputDevice { device, reply } => {
                                audio.set_input_device(device);
                                drop(reply.send(Ok(())));
                            }
                        },
                        // Idle: check whether the world of sound changed.
                        Err(RecvTimeoutError::Timeout) => {
                            if audio.default_output_changed() {
                                audio.rebuild_output();
                            }
                            if audio.default_input_changed() {
                                audio.rebuild_mics();
                            }
                            // Revive microphones whose stream could not be
                            // opened before, and reopen a lost output.
                            if audio.mics.values().any(|slot| slot.stream.is_none()) {
                                audio.rebuild_mics();
                            }
                            if audio.output.is_none() && audio.frame_players_open() {
                                audio.rebuild_output();
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .context("Could not spawn the audio thread")?;

        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self { cmd_tx }),
            Ok(Err(error)) => Err(anyhow!("Could not connect to the sound device: {error}")),
            Err(_) => Err(anyhow!("The audio thread died before it could start")),
        }
    }

    /// Creates a new player for live audio, like one call participant.
    pub async fn frame_player(&self) -> Result<FramePlayer> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::FramePlayer { reply: reply_tx })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }

    /// Plays an audio file.
    pub async fn play_file(&self, path: impl Into<PathBuf>) -> Result<Sound> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::PlayFile {
                path: path.into(),
                reply: reply_tx,
            })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }

    /// Plays audio data that is already in memory.
    pub async fn play_data(&self, data: impl Into<Vec<u8>>) -> Result<Sound> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::PlayData {
                data: data.into(),
                reply: reply_tx,
            })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }

    /// Plays an already-decoded source.
    pub async fn play_source<S>(&self, source: S) -> Result<Sound>
    where
        S: Source<Item = f32> + Send + Sync + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::PlaySource {
                source: Box::new(source),
                reply: reply_tx,
            })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }

    /// Starts recording the default microphone, drop the handle to stop it.
    pub async fn microphone(&self) -> Result<Microphone> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::Microphone { reply: reply_tx })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }

    /// Switches the sounds to another output device, or back to the
    /// system default when `device` is `None`. Live call audio survives
    /// the switch, already playing one-shot sounds stop.
    pub async fn set_output_device(&self, device: Option<cpal::Device>) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::SetOutputDevice {
                device,
                reply: reply_tx,
            })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }

    /// Switches recording to another input device, or back to the system
    /// default when `device` is `None`. Open microphones keep working.
    pub async fn set_input_device(&self, device: Option<cpal::Device>) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::SetInputDevice {
                device,
                reply: reply_tx,
            })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }
}

/// A sound device as a settings menu sees it. The name is shown, the
/// device is handed back when switching devices.
pub struct DeviceDescription {
    pub name: String,
    /// `None` means "use the system default".
    pub device: Option<cpal::Device>,
}

impl DeviceDescription {
    /// The "follow the system default" choice for a device list.
    pub fn default_choice() -> Self {
        Self {
            name: "System default".into(),
            device: None,
        }
    }
}

/// Every output device that exists right now, plus the default choice.
pub fn output_devices() -> Vec<DeviceDescription> {
    let mut list = vec![DeviceDescription::default_choice()];
    if let Ok(devices) = cpal::default_host().output_devices() {
        list.extend(devices.filter_map(|device| {
            let name = device.name().ok()?;
            Some(DeviceDescription {
                name,
                device: Some(device),
            })
        }));
    }
    list
}

/// Every input device that exists right now, plus the default choice.
pub fn input_devices() -> Vec<DeviceDescription> {
    let mut list = vec![DeviceDescription::default_choice()];
    if let Ok(devices) = cpal::default_host().input_devices() {
        list.extend(devices.filter_map(|device| {
            let name = device.name().ok()?;
            Some(DeviceDescription {
                name,
                device: Some(device),
            })
        }));
    }
    list
}
/// A running microphone recording, drop the handle to stop it.
pub struct Microphone {
    /// Which stream this handle belongs to.
    id: u64,
    samples: Arc<MicSamples>,
    /// To tell the audio thread this handle is gone.
    cmd_tx: std::sync::mpsc::Sender<Command>,
}

impl Drop for Microphone {
    /// Stops the recording when the last handle goes away.
    fn drop(&mut self) {
        drop(self.cmd_tx.send(Command::StopMicrophone { id: self.id }));
    }
}

impl Microphone {
    /// Takes everything recorded since the last call, empty if nothing is new.
    pub fn pop(&self) -> Vec<f32> {
        let mut queue = self
            .samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        queue.drain(..).collect()
    }

    /// Throws away everything recorded but not yet read.
    pub fn clear(&self) {
        self.samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Stops buffering new audio until this is called with false.
    pub fn set_muted(&self, muted: bool) {
        self.samples.muted.store(muted, Ordering::Relaxed);
        if muted {
            self.clear();
        }
    }

    /// Whether audio is currently being ignored instead of buffered.
    pub fn is_muted(&self) -> bool {
        self.samples.muted.load(Ordering::Relaxed)
    }

    /// How often incoming sound had to be thrown away because it was not read fast enough.
    pub fn overflows(&self) -> u64 {
        self.samples.overflows.load(Ordering::Relaxed)
    }
}

/// The buffer cpal fills and the microphone handle reads from.
#[derive(Default)]
struct MicSamples {
    queue: Mutex<VecDeque<f32>>,
    /// True while recording is paused and incoming sound is thrown away.
    muted: AtomicBool,
    overflows: AtomicU64,
}

/// Builds the cpal capture stream that fills the buffer with whatever the device says.
fn build_mic_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    samples: Arc<MicSamples>,
) -> Result<cpal::Stream> {
    let channels = config.channels().max(1) as usize;
    let sample_rate = config.sample_rate().0;
    let error = move |error| event!(Level::WARN, "The microphone misbehaved: {error}");

    // One closure per sample format, all ending up in the same mono f32 queue.
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.config(),
            move |data: &[f32], _| push_mic_chunk(data, channels, sample_rate, &samples),
            error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.config(),
            move |data: &[i16], _| {
                let chunk: Vec<f32> = data
                    .iter()
                    .map(|&sample| f32::from(sample) / f32::from(i16::MAX))
                    .collect();
                push_mic_chunk(&chunk, channels, sample_rate, &samples);
            },
            error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config.config(),
            move |data: &[u16], _| {
                let chunk: Vec<f32> = data
                    .iter()
                    .map(|&sample| (f32::from(sample) - 32768.0) / 32768.0)
                    .collect();
                push_mic_chunk(&chunk, channels, sample_rate, &samples);
            },
            error,
            None,
        ),
        other => {
            return Err(anyhow!(
                "The microphone speaks an unsupported format: {other:?}"
            ));
        }
    }
    .context("Could not open the microphone")?;

    Ok(stream)
}

/// Mixes one captured chunk down to mono 48 kHz and buffers it.
fn push_mic_chunk(chunk: &[f32], channels: usize, sample_rate: u32, samples: &Arc<MicSamples>) {
    if samples.muted.load(Ordering::Relaxed) || chunk.is_empty() {
        return;
    }

    let mono: Vec<f32> = chunk
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect();

    let converted = resample(&mono, sample_rate);

    let mut queue = samples
        .queue
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Drop the oldest samples if too much piles up, so a microphone nobody
    // reads from cannot eat all the memory.
    if queue.len() + converted.len() > MAX_BUFFERED_SAMPLES {
        let excess = (queue.len() + converted.len()).saturating_sub(MAX_BUFFERED_SAMPLES);
        let to_drop = excess.min(queue.len());
        queue.drain(..to_drop);
        samples.overflows.fetch_add(1, Ordering::Relaxed);
    }

    queue.extend(converted);
}

/// A player for live audio, like one participant in a call.
///
/// Push received frames into it and the player handles the rest. Frames
/// are mixed down to stereo and resampled to 48 kHz.
pub struct FramePlayer {
    /// Which sink-slots this handle belongs to, on the audio thread.
    id: u64,
    samples: Arc<SharedSamples>,
    /// To tell the audio thread this handle is gone.
    cmd_tx: std::sync::mpsc::Sender<Command>,
}

impl Drop for FramePlayer {
    /// Stops the call audio when the last handle goes away.
    fn drop(&mut self) {
        drop(self.cmd_tx.send(Command::StopFramePlayer { id: self.id }));
    }
}

impl FramePlayer {
    /// Hands one received frame to the player. That is all you ever have to
    /// do, the player takes care of the rest. Cheap and never blocks, so
    /// call it the moment a frame arrives.
    pub fn push(&self, frame: &AudioFrame<'_>) {
        let converted = frame_to_stereo_48k(frame);
        let mut queue = self
            .samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Drop the oldest samples if too much piles up, capping the delay
        // the listener hears behind the actual call.
        if queue.len() + converted.len() > MAX_BUFFERED_SAMPLES {
            let excess = (queue.len() + converted.len()).saturating_sub(MAX_BUFFERED_SAMPLES);
            let to_drop = excess.min(queue.len());
            queue.drain(..to_drop);
            self.samples.overflows.fetch_add(1, Ordering::Relaxed);
        }

        queue.extend(converted);
    }

    /// Throws away everything buffered but not yet played. Use it when
    /// leaving a call, so the next call does not start with old audio.
    pub fn clear(&self) {
        self.samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// How many milliseconds of audio sit in the buffer, how far the
    /// played sound lags behind what was received. Handy for debugging.
    pub fn buffered_millis(&self) -> u64 {
        let len = self
            .samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();

        len as u64 * 1000 / (OUTPUT_SAMPLE_RATE as u64 * OUTPUT_CHANNELS as u64)
    }

    /// How often the sound device wanted samples and the buffer was empty.
    pub fn underruns(&self) -> u64 {
        self.samples.underruns.load(Ordering::Relaxed)
    }

    /// How often incoming frames had to be thrown away because the buffer
    /// was full.
    pub fn overflows(&self) -> u64 {
        self.samples.overflows.load(Ordering::Relaxed)
    }

    /// Sets how loud they play. 0.0 is silent, 1.0 is normal, anything
    /// above boosts it.
    ///
    /// Survives the sink being recreated after a device switch.
    pub fn set_volume(&self, volume: f32) {
        *self
            .samples
            .volume
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = volume;
    }

    /// Pauses this player until it is played again.
    pub fn pause(&self) {
        self.samples.paused.store(true, Ordering::Relaxed);
    }

    /// Resumes a paused player.
    pub fn play(&self) {
        self.samples.paused.store(false, Ordering::Relaxed);
    }
}

/// The buffer frames get pushed into and the sound device's thread reads out
/// of. Shared between whoever pushes and the playback thread.
#[derive(Default)]
struct SharedSamples {
    queue: Mutex<VecDeque<f32>>,
    /// True while the buffer is empty and silence is being played
    underrunning: AtomicBool,
    underruns: AtomicU64,
    overflows: AtomicU64,
    /// Volume applied by the player itself, `1.0` normal, so it survives
    /// the sink being recreated after a device switch.
    volume: Mutex<f32>,
    /// True while playback is paused and silence is played instead.
    paused: AtomicBool,
}

/// Turns a LiveKit frame into f32 stereo samples at 48 kHz, what the
/// sound device wants. LiveKit normally already sends exactly that, so
/// this is mostly a type conversion.
fn frame_to_stereo_48k(frame: &AudioFrame<'_>) -> Vec<f32> {
    let channels = frame.num_channels.max(1) as usize;
    let samples: Vec<f32> = frame
        .data
        .iter()
        .map(|&sample| f32::from(sample) / f32::from(i16::MAX))
        .collect();

    if samples.len() != channels * frame.samples_per_channel.max(1) as usize {
        // Malformed frame, skipping it keeps a 10 ms hole instead
        event!(
            Level::WARN,
            "Skipping audio frame with wrong size: {} samples, {channels} channels",
            samples.len()
        );
        return Vec::new();
    }

    // To stereo: mono is played on both ears, more than two channels keep
    // the first two.
    let stereo: Vec<f32> = match channels {
        1 => samples.iter().flat_map(|&s| [s, s]).collect(),
        2 => samples,
        _ => samples
            .as_chunks::<2>()
            .0
            .iter()
            .flatten()
            .copied()
            .collect(),
    };

    resample(&stereo, frame.sample_rate)
}

/// Stretches or squeezes samples to exactly 48 kHz. Frames normally are
/// 48 kHz already, so this is almost always just a copy for the rare
/// source that sends something else.
fn resample(samples: &[f32], from_rate: u32) -> Vec<f32> {
    if from_rate == OUTPUT_SAMPLE_RATE || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = f64::from(OUTPUT_SAMPLE_RATE) / f64::from(from_rate);
    let out_len = (samples.len() as f64 * ratio) as usize;
    let step = f64::from(from_rate) / f64::from(OUTPUT_SAMPLE_RATE);

    (0..out_len)
        .map(|out_index| {
            // Mix the position between two neighboring samples to guess the
            // value at the new rate from the old ones.
            let position = out_index as f64 * step;
            let left = (position.floor() as usize).min(samples.len() - 1);
            let right = (left + 1).min(samples.len() - 1);
            let blend = position.fract() as f32;

            samples[left] * (1.0 - blend) + samples[right] * blend
        })
        .collect()
}

/// Feeds the sound device from the shared buffer and plays silence when
/// nothing is there. It never ends on purpose, a call player should not
/// die just because it ran dry.
struct FrameSource {
    samples: Arc<SharedSamples>,
}

impl Iterator for FrameSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.samples.paused.load(Ordering::Relaxed) {
            return Some(0.0);
        }

        let volume = *self
            .samples
            .volume
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut queue = self
            .samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match queue.pop_front() {
            Some(sample) => {
                // The underrun is over once real data is flowing again.
                self.samples.underrunning.store(false, Ordering::Relaxed);
                Some(sample * volume)
            }
            None => {
                // Nothing buffered: play silence and count the heart stroke once.
                if !self.samples.underrunning.swap(true, Ordering::Relaxed) {
                    self.samples.underruns.fetch_add(1, Ordering::Relaxed);
                }
                Some(0.0)
            }
        }
    }
}

impl Source for FrameSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        OUTPUT_CHANNELS
    }

    fn sample_rate(&self) -> u32 {
        OUTPUT_SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
