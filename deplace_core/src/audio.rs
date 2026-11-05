//! Playing any and all sounds (only here).
//!
//! This module is the one thing that talks to the sound device.
//! It uses rodio, which works on Linux, Windwos and (macOS)
//! Everything is mixed inside one mixer thingemagick.
//!
//! There are two ways to make noise:
//!
//! - [`Audio::play_file`] / [`Audio::play_data`] for finished sounds
//!   They start playing immediately and return a
//!   [`Sound`] handle that stops the sound when dropped.
//! - [`Audio::frame_player`] for
//!   call audio from LiveKit. Hand the [`FramePlayer`]
//!   every [`AudioFrame`]
//!
//! And one way to listen:
//!
//! - [`AudioThread::microphone`] for recording
//!   whatever the default microphone hears, frame by frame in a [`Microphone`]
//!
//! We'll get soundboard things too just you wait

use std::{
    collections::{HashMap, VecDeque},
    io::{BufReader, Cursor},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use livekit::webrtc::prelude::AudioFrame;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use tokio::sync::oneshot;
use tracing::{Level, event};

/// Livekit uses this if I remember right
const OUTPUT_SAMPLE_RATE: u32 = 48_000;
/// Stereo ONLY!!!! >:)
const OUTPUT_CHANNELS: u16 = 2;
/// For 48Hz that's like 1/4th of a second or smth not like I study math
const MAX_BUFFERED_SAMPLES: usize = 12_000;

/// Connection to the sound device.
///
/// Create one `Audio` when the app starts and keep it alive until the app
/// closesCwhile it exists, sounds can be played. Creating it opens the
/// default output device so I might need to do some more stuff
pub struct Audio {
    /// dropping it would silence everything.
    _output: Box<OutputStream>,
    handle: OutputStreamHandle,
    /// Microphone capture streams, one per open [`Microphone`].
    mics: HashMap<u64, cpal::Stream>,
    /// Id the next created [`Microphone`] gets.
    next_mic_id: u64,
}

impl Audio {
    /// Connects to the default sound output device.
    pub fn new() -> Result<Self> {
        let (output, handle) = OutputStream::try_default()
            .context("Could not open the default sound output device")?;

        Ok(Self {
            _output: Box::new(output),
            handle,
            mics: HashMap::new(),
            next_mic_id: 0,
        })
    }

    /// What do you think it does?
    pub fn play_file<P: AsRef<Path>>(&self, path: P) -> Result<Sound> {
        let file = std::fs::File::open(path).context("Could not open the audio file")?;

        let sink = self
            .handle
            .play_once(BufReader::new(file))
            .context("Could not decode/play the audio file")?;

        Ok(Sound { sink })
    }

    /// Play any audio data
    pub fn play_data(&self, data: impl Into<Vec<u8>>) -> Result<Sound> {
        let sink = self
            .handle
            .play_once(Cursor::new(data.into()))
            .context("Could not decode/play the audio data")?;

        Ok(Sound { sink })
    }

    /// Creates a new player for live audio that arrives frame by frame
    /// Each player is mixed in with everything
    /// else and can be dropped to stop it.
    pub fn frame_player(&self) -> Result<FramePlayer> {
        let sink = Sink::try_new(&self.handle).context("Could not create a frame player")?;

        let samples = Arc::new(SharedSamples::default());
        sink.append(FrameSource {
            samples: Arc::clone(&samples),
        });

        Ok(FramePlayer { sink, samples })
    }

    /// Starts recording the default microphone, drop the handle to stop it.
    fn microphone(&mut self, cmd_tx: &std::sync::mpsc::Sender<Command>) -> Result<Microphone> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("There is no default microphone")?;

        let config = device
            .default_input_config()
            .context("Could not read the microphone's format")?;

        let samples = Arc::new(MicSamples::default());
        let stream = build_mic_stream(&device, &config, Arc::clone(&samples))?;
        stream.play().context("Could not start the microphone")?;

        let id = self.next_mic_id;
        self.next_mic_id += 1;
        self.mics.insert(id, stream);

        Ok(Microphone {
            id,
            samples,
            cmd_tx: cmd_tx.clone(),
        })
    }

    /// Stops the capture stream belonging to a dropped [`Microphone`].
    fn stop_microphone(&mut self, id: u64) {
        self.mics.remove(&id);
    }
}

/// A finished sound that is playing right now.
///
/// Returned by [`Audio::play_file`] and [`Audio::play_data`]. The sound keeps
/// playing until the handle is dropped or it ends ig
pub struct Sound {
    sink: Sink,
}

impl Sound {
    /// Sets how loud this sound is: `0.0` is silent, `1.0` is normal volume, anything
    /// else just multiplies the sound by the volume blah blah blah you get it
    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    /// Pauses this sound until [`Sound::play`] is called. no rush.
    pub fn pause(&self) {
        self.sink.pause();
    }

    /// Resumes this sound after [`Sound::pause`].
    pub fn play(&self) {
        self.sink.play();
    }

    /// Stops the sound immediately
    pub fn stop(self) {
        // Dropping the sink stops playback so we just drop self
        // I'm so proud of this
    }
}

/// [`Audio`] but usable from async code.
///
/// [`Audio`] is not allowed to cross threads (thanks cpal), but
/// basically everything in this app happens in async tasks. So this puts
/// the real `Audio` on its own little thread and passes requests to it
/// through a channel. From the outside it just looks like the normal thing.
/// Something something quacks like a duck something someting
///
/// Keep one alive as long as you want to make sounds.
#[derive(Clone)]
pub struct AudioThread {
    /// The end of the channel the little audio thread listens to. As long
    /// as this lives, the thread lives.
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
    Microphone {
        reply: oneshot::Sender<Result<Microphone>>,
    },
    StopMicrophone {
        id: u64,
    },
}

impl AudioThread {
    /// Starts the little audio thread and connects to the sound device.
    /// Do this once and keep the handle around, clone it wherever needed.
    pub fn start() -> Result<Self> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        // Just used to find out whether the connection worked, the thread
        // answers actual requests through the oneshots in the commands.
        let (init_tx, init_rx) = std::sync::mpsc::channel();
        // Cloned into every Microphone so it can announce its own death.
        let stop_tx = cmd_tx.clone();

        std::thread::Builder::new()
            .name("deplace-audio".into())
            .spawn(move || {
                let mut audio = match Audio::new() {
                    Ok(audio) => audio,
                    Err(e) => {
                        // Tell start() it did not work and die quietly.
                        drop(init_tx.send(Err(format!("{e:#}"))));
                        return;
                    }
                };

                drop(init_tx.send(Ok(())));

                // Answer requests until everyone is gone (last handle
                // dropped), then the iteration ends and so does the thread.
                // Nobody listening to the answer anymore is fine, the
                // sound happens either way. Or does it? I'm going crazy over here
                for command in cmd_rx {
                    match command {
                        Command::FramePlayer { reply } => {
                            drop(reply.send(audio.frame_player()));
                        }
                        Command::PlayFile { path, reply } => {
                            drop(reply.send(audio.play_file(path)));
                        }
                        Command::PlayData { data, reply } => {
                            drop(reply.send(audio.play_data(data)));
                        }
                        Command::Microphone { reply } => {
                            drop(reply.send(audio.microphone(&stop_tx)));
                        }
                        Command::StopMicrophone { id } => {
                            audio.stop_microphone(id);
                        }
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
    /// Same as [`Audio::frame_player`], just async
    pub async fn frame_player(&self) -> Result<FramePlayer> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.cmd_tx
            .send(Command::FramePlayer { reply: reply_tx })
            .context("The audio thread is gone")?;

        reply_rx
            .await
            .context("The audio thread died mid-request")?
    }

    /// Plays an audio file. Same as [`Audio::play_file`], just
    /// async
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

    /// Plays audio data that is in memory. Same as [`Audio::play_data`],
    /// just async
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
}

/// The microphone of the machine
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

    /// Stops buffering new audio until [`Microphone::set_muted`] is called with false.
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

/// The buffer the microphone thread fills and [`Microphone`] reads from.
#[derive(Default)]
struct MicSamples {
    queue: Mutex<VecDeque<f32>>,
    /// True while recording is paused and incoming sound is thrown away.
    muted: AtomicBool,
    overflows: AtomicU64,
}

/// Builds the cpal capture stream that fills a [`MicSamples`] with whatever the device says.
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

/// Mixes one captured chunk down to mono 48 kHz and buffers it in a [`MicSamples`].
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

/// A player for live audio like one participant in a LiveKite call.
///
/// Hand [`AudioFrame`]s over with
/// [`FramePlayer::push`] and the player does the rest.
///
/// Frames get mixed down to stereo and resampled to 48 kHz if they come in
/// with some weird other format, but LiveKit already sends 48 kHz, so maybe
/// I should log it or something
pub struct FramePlayer {
    sink: Sink,
    samples: Arc<SharedSamples>,
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

    /// How many milliseconds of audio are sitting in the buffer right now,
    /// how far the played sound lags behind the received data.
    ///
    /// This will be great for debugging or something trust trust
    pub fn buffered_millis(&self) -> u64 {
        let len = self
            .samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();

        len as u64 * 1000 / (OUTPUT_SAMPLE_RATE as u64 * OUTPUT_CHANNELS as u64)
    }

    /// How often the sound device wanted samples and the buffer was empty
    /// Sure something interesting to know right? Like no output or smth
    pub fn underruns(&self) -> u64 {
        self.samples.underruns.load(Ordering::Relaxed)
    }

    /// How often incoming frames had to be thrown away because the buffer
    /// was full. This is more intereisting right?
    pub fn overflows(&self) -> u64 {
        self.samples.overflows.load(Ordering::Relaxed)
    }

    /// How loud this player is: `0.0` is silent, `1.0` is normal volume
    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    /// Pauses this player until [`FramePlayer::play`] is called.
    pub fn pause(&self) {
        self.sink.pause();
    }

    /// Resumes this player after [`FramePlayer::pause`].
    pub fn play(&self) {
        self.sink.play();
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
}

/// Turns a LiveKit frame into f32 stereo samples at [`OUTPUT_SAMPLE_RATE`],
/// which is what the sound device wants. LiveKit normally already sends
/// exactly that, so this is basically just a type conversion buuut just in
/// case I was wrong or somethign
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

/// Stretches or squeezes samples to exactly [`OUTPUT_SAMPLE_RATE`]. Frames
/// normally are 48 kHz already so this is almost always just a copy, it
/// exists for the rare source that sends something else, still unsure
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

/// The part the sound device drains: [`SharedSamples`], or
/// silence if there is nothing there, forever. It never ends on purpose, a
/// call player should not just die because someone had a stroke.
struct FrameSource {
    samples: Arc<SharedSamples>,
}

impl Iterator for FrameSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let mut queue = self
            .samples
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match queue.pop_front() {
            Some(sample) => {
                // The underrun is over once real data is flowing again.
                self.samples.underrunning.store(false, Ordering::Relaxed);
                Some(sample)
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
