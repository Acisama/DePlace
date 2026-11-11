//! Streams video files into a gstreamer pipeline fed by `appsrc`.
//!
//! This lets a video be played directly from its encrypted representation on
//! disk: the Matrix attachment decryptor runs in-memory over the file as
//! gstreamer pulls data, so the decrypted video is never written to disk.
//!
//! Seeking (which mp4 demuxers do, e.g. for `moov` at the end of the file or
//! when looping) is supported by repositioning the decrypted stream: the
//! AES-CTR keystream is addressable by byte offset, so a seek is a counter
//! jump in constant time -- no matter how far into a big file it goes.

use std::{
    io::Read,
    path::Path,
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
    thread::JoinHandle,
};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use iced_video_player::Video;
use ruma::events::room::EncryptedFile;

/// How much data the appsrc queue may hold before push blocks. Sizing this
/// moderately keeps seeks responsive: a blocked feed unblocks when the demuxer
/// flushes.
const MAX_QUEUE_BYTES: u64 = 8 * 1024 * 1024;
/// Size of the chunks pulled from disk / the decryptor.
const CHUNK_SIZE: usize = 512 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum VideoStreamError {
    #[error("gstreamer error: {0}")]
    Glib(#[from] gst::glib::Error),
    #[error("failed to create gstreamer elements: {0}")]
    Element(#[from] gst::glib::BoolError),
    #[error("pipeline element cast failed")]
    Cast,
    #[error("failed to decrypt video: {0}")]
    Decrypt(#[from] matrix_sdk_base::crypto::DecryptorError),
    #[error("failed to open video file: {0}")]
    Io(String),
    #[error("failed to play video: {0}")]
    Play(#[from] iced_video_player::Error),
}

impl From<std::io::Error> for VideoStreamError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Signals sent to the feeding thread.
enum FeedSignal {
    /// Reposition the stream to the given byte offset and keep pushing.
    /// Carries the acknowledger of the `seek_data` callback, which blocks
    /// until the feeder repositioned: returning `true` from the callback
    /// promises gstreamer that data arrives at the requested offset, so the
    /// seek must actually be serviced before it returns.
    Seek(u64, std::sync::mpsc::Sender<()>),
}

/// Holds a sender so the feeding thread keeps running while the returned
/// handle is alive; the pipeline's `seek-data` callback holds another one.
#[derive(Debug)]
struct StreamFeederInner {
    _tx: Sender<FeedSignal>,
    _handle: JoinHandle<()>,
}

/// Handle that must be kept alive alongside a streaming [`Video`]; dropping
/// all of them stops the feeding thread.
#[derive(Clone)]
/// Debug only for the cache derive machinery.
#[derive(Debug)]
pub struct StreamFeeder(#[allow(dead_code)] Arc<StreamFeederInner>);

/// Keeps the gstreamer log forwarder alive once installed.
static GST_LOG_FORWARDER: std::sync::OnceLock<gst::log::DebugLogFunction> =
    std::sync::OnceLock::new();

/// Forwards gstreamer's internal warning/error log lines into our tracing
/// log. Gstreamer's own default handler writes them to stderr, which our
/// log file doesn't capture -- and its warnings are the only place naming
/// the element whose state change failed when a pipeline breaks.
fn init_gst_log_forwarding() {
    if GST_LOG_FORWARDER.get().is_some() {
        return;
    }

    let forwarder = gst::log::add_log_function(
        |_category, level, file, function_name, line, object, message| {
            if level > gst::DebugLevel::Warning {
                return;
            }

            let location = format!("{file}:{line} in {function_name}");
            let object = object.map(|object| object.to_string());
            let message = message
                .get()
                .map(|message| message.to_string())
                .unwrap_or_default();

            match level {
                gst::DebugLevel::Error => {
                    tracing::error!("[gst {location} {object:?}] {message}");
                }
                _ => {
                    tracing::warn!("[gst {location} {object:?}] {message}");
                }
            }
        },
    );

    // The result of `set` is irrelevant: when the forwarder was already
    // installed, the freshly created one simply replaces the previous
    // registration and the previous handle is released.
    if let Err(previous) = GST_LOG_FORWARDER.set(forwarder) {
        gst::log::remove_log_function(previous);
    }
}

/// Logs the first bytes of the decrypted stream as a diagnostic probe:
/// container formats are recognizable (mp4 starts with a box length and
/// `ftyp`, followed by the `moov` box for faststart files), so the log
/// output immediately shows whether the data fed to gstreamer is actually
/// the video file, and if not, at which byte the corruption starts.
fn log_decrypted_head(_path: &Path, _encryption: Option<&EncryptedFile>) {
    //     const HEAD_LEN: usize = 512;
    //
    //     let mut head = [0u8; HEAD_LEN];
    //     let head: Vec<u8> = match encryption {
    //         Some(encrypted) => {
    //             let Ok(mut plain) = std::fs::File::open(path) else {
    //                 tracing::warn!(stream_head = "unreadable", "video stream head");
    //                 return;
    //             };
    //             let Ok(mut decryptor) = matrix_sdk_base::crypto::AttachmentDecryptor::new(
    //                 &mut plain,
    //                 encrypted.clone().into(),
    //             ) else {
    //                 tracing::warn!(stream_head = "undecryptable", "video stream head");
    //                 return;
    //             };
    //             if decryptor.read_exact(&mut head).is_err() {
    //                 tracing::warn!(stream_head = "unreadable", "video stream head");
    //                 return;
    //             }
    //             head.to_vec()
    //         }
    //         None => return,
    //     };

    // let hex: String = head.iter().map(|byte| format!("{byte:02x}")).collect();
    // tracing::info!(head_bytes = %hex, "decrypted video stream head");
}

/// Opens a video from a file, decrypting it in-memory on the fly if it is a
/// Matrix encrypted attachment.
pub fn open_stream(
    path: &Path,
    encryption: Option<&EncryptedFile>,
    audio_sink: &str,
) -> Result<(Video, StreamFeeder), VideoStreamError> {
    gst::init()?;
    init_gst_log_forwarding();

    let file_size = std::fs::metadata(path)?.len();
    log_decrypted_head(path, encryption);

    let pipeline = gst::Pipeline::with_name("deplace-stream");
    let appsrc = gst::ElementFactory::make("appsrc")
        .name("videosrc")
        .build()?
        .downcast::<gst_app::AppSrc>()
        .map_err(|_| VideoStreamError::Cast)?;

    // Byte-based, seekable and with blocking pushes: gstreamer pulls data at
    // its own pace and reports seeks as byte offsets, which map 1:1 onto the
    // (encrypted) file.
    appsrc.set_format(gst::Format::Bytes);
    appsrc.set_stream_type(gst_app::AppStreamType::Seekable);
    appsrc.set_block(true);
    appsrc.set_max_bytes(MAX_QUEUE_BYTES);
    appsrc.set_size(file_size as i64);

    let decodebin = gst::ElementFactory::make("decodebin")
        .name("decoder")
        .build()?;

    // The video sink chain mirrors what `Video::new` builds for playbin; a
    // queue decouples the decoder thread from the (synced) sink.
    let video_bin = gst::parse::bin_from_description(
        "queue ! videoscale ! videoconvert ! appsink name=iced_video drop=true sync=true caps=video/x-raw,format=NV12,pixel-aspect-ratio=1/1",
        true,
    )?;

    // Audio and text sinks are added statically like the video one --
    // creating sink bins in `pad-added` races the incoming data. Files
    // without those streams simply leave the corresponding chain unlinked
    // until it is removed below: that must not block the pipeline state
    // change, so the actual sinks (autoaudiosink is a wrapper bin, the
    // property must be set on the inner element) get `async=false`.
    let audio_bin = gst::parse::bin_from_description(
        &format!("queue ! audioconvert ! audioresample ! {audio_sink}"),
        true,
    )?;
    for element in audio_bin.iterate_recurse() {
        let Ok(element) = element else {
            continue;
        };
        if element.has_property("async", None) {
            element.set_property("async", false);
        }
    }
    let text_bin = gst::parse::bin_from_description("queue ! fakesink", true)?;
    for element in text_bin.iterate_recurse() {
        let Ok(element) = element else {
            continue;
        };
        if element.has_property("async", None) {
            element.set_property("async", false);
        }
    }

    let appsrc_element = appsrc.upcast_ref::<gst::Element>();
    pipeline.add_many([
        appsrc_element,
        decodebin.upcast_ref(),
        video_bin.upcast_ref(),
        audio_bin.upcast_ref(),
        text_bin.upcast_ref(),
    ])?;
    appsrc_element.link(&decodebin)?;

    // Which sink chains (video/audio/text) actually have a decoded stream
    // linked onto them.
    let linked_kinds: Arc<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));

    // Route the decoded pads to the statically created sink chains.
    {
        let video_bin = video_bin.clone();
        let audio_bin = audio_bin.clone();
        let text_bin = text_bin.clone();
        let linked_kinds = linked_kinds.clone();

        decodebin.connect_pad_added(move |_, pad| {
            let media_type = pad
                .current_caps()
                .and_then(|caps| caps.structure(0).map(|s| s.name().to_string()));

            let Some(media_type) = media_type.as_deref() else {
                return;
            };

            let (kind, target) = if media_type.starts_with("video") {
                ("video", video_bin.static_pad("sink"))
            } else if media_type.starts_with("audio") {
                ("audio", audio_bin.static_pad("sink"))
            } else if media_type.starts_with("text") || media_type.starts_with("subtitle") {
                ("text", text_bin.static_pad("sink"))
            } else {
                tracing::debug!("Skipping unhandled decoded pad of kind {media_type}");
                return;
            };

            match target {
                Some(target) => match pad.link(&target) {
                    Ok(_) => {
                        linked_kinds.lock().expect("linked kinds lock").insert(kind);
                    }
                    Err(e) => tracing::warn!("Failed to link decoded pad: {e}"),
                },
                None => {
                    tracing::warn!("Sink chain of a {media_type} pad is missing its sink pad")
                }
            }
        });
    }

    // Sink chains that ended up unused are removed once the stream
    // composition is known: an unlinked sink inside the pipeline is a dead
    // end that makes every pipeline-level seek (and thus looping) fail.
    {
        let pipeline = pipeline.clone();
        let audio_bin = audio_bin.clone();
        let text_bin = text_bin.clone();
        let linked_kinds = linked_kinds.clone();

        decodebin.connect_no_more_pads(move |_| {
            let linked = linked_kinds.lock().expect("linked kinds lock");

            for (kind, bin) in [("audio", &audio_bin), ("text", &text_bin)] {
                if !linked.contains(kind)
                    && let Err(e) = pipeline.remove(bin)
                {
                    tracing::warn!("Failed to remove unused {} sink chain: {e}", kind);
                }
            }
        });
    }

    let (tx, rx) = channel::<FeedSignal>();
    let callback_tx = tx.clone();

    appsrc.set_callbacks(
        gst_app::AppSrcCallbacks::builder()
            .seek_data(move |_, offset| {
                // Block until the feeding thread repositioned the stream.
                let (ack_tx, ack_rx) = channel::<()>();
                callback_tx.send(FeedSignal::Seek(offset, ack_tx)).is_ok() && ack_rx.recv().is_ok()
            })
            .build(),
    );

    // Spawn the feeding thread before the pipeline starts, so data is
    // available while the player waits for the decoder caps.
    let feed_appsrc = appsrc.clone();
    let feed_path = path.to_path_buf();
    let feed_encryption = encryption.cloned();
    let handle = std::thread::Builder::new()
        .name("deplace-video-stream".into())
        .spawn(move || {
            feed(feed_appsrc, &rx, &feed_path, feed_encryption.as_ref());
        })
        .map_err(|e| VideoStreamError::Io(e.to_string()))?;

    let video_appsink = video_bin
        .by_name("iced_video")
        .and_downcast::<gst_app::AppSink>()
        .ok_or(VideoStreamError::Cast)?;

    // Keep the bus for the failure diagnostics below.
    let bus = pipeline.bus().ok_or(VideoStreamError::Cast)?;
    let video = match Video::from_gst_pipeline(pipeline, video_appsink, None) {
        Ok(video) => video,
        Err(e) => {
            // The player only reports "failed to change state"; the bus
            // carries the actual error naming the failing element.
            while let Some(message) = bus.pop() {
                let view = message.view();
                if let gst::MessageView::Error(error) = view {
                    tracing::error!(
                        "gstreamer pipeline error from {}: {} ({})",
                        message.src().map(|src| src.to_string()).unwrap_or_default(),
                        error.error(),
                        error.debug().unwrap_or_default()
                    );
                }
            }
            return Err(e.into());
        }
    };

    Ok((
        video,
        StreamFeeder(Arc::new(StreamFeederInner {
            _tx: tx,
            _handle: handle,
        })),
    ))
}

/// Main loop of the feeding thread: streams decrypted chunks into the appsrc
/// until end-of-stream, then parks until a seek revitalizes it.
fn feed(
    appsrc: gst_app::AppSrc,
    rx: &Receiver<FeedSignal>,
    path: &Path,
    encryption: Option<&EncryptedFile>,
) {
    let mut offset = 0;

    loop {
        match stream_run(&appsrc, rx, path, offset, encryption) {
            // A seek arrived mid-stream: the pump already acknowledged it and
            // the run continues at the offset it set.
            Ok(FeedOutcome::SeekAt(seek)) => offset = seek,
            // The stream ended: park until the next seek arrives (the
            // appsrc's `seek_data` callback blocks on its acknowledgment).
            Ok(FeedOutcome::Eos) => match rx.recv() {
                Ok(FeedSignal::Seek(seek, ack)) => {
                    if ack.send(()).is_err() {
                        return;
                    }
                    offset = seek;
                }
                // All senders dropped: the pipeline is gone.
                Err(_) => return,
            },
            Err(e) => {
                tracing::error!("Video stream feeder failed: {e}");
                return;
            }
        }
    }
}

enum FeedOutcome {
    /// The stream ended; park until a seek arrives.
    Eos,
    /// A seek was acknowledged mid-stream; continue from this offset.
    SeekAt(u64),
}

/// Streams from the file at the given offset until end-of-stream or a seek.
fn stream_run(
    appsrc: &gst_app::AppSrc,
    rx: &Receiver<FeedSignal>,
    path: &Path,
    offset: u64,
    encryption: Option<&EncryptedFile>,
) -> Result<FeedOutcome, VideoStreamError> {
    let mut file = std::fs::File::open(path)?;

    // Both kinds of stream are seekable directly: plain files via their
    // offset, encrypted ones by repositioning the CTR keystream counter --
    // no data has to pass through the decryptor to get to the requested
    // byte, making a seek to a far-off offset (e.g. the `moov` box at the
    // end of a big non-faststart file) effectively instant.
    match encryption {
        None => {
            use std::io::Seek as _;
            file.seek(std::io::SeekFrom::Start(offset))?;
            pump(appsrc, rx, &mut file)
        }
        Some(encrypted) => {
            let mut reader = crate::state::local_file_encryption::AttachmentReader::new(
                file,
                encrypted.clone().into(),
            )?;
            reader.seek_to(offset)?;
            pump(appsrc, rx, &mut reader)
        }
    }
}

/// Pushes chunks from the stream into the appsrc until the file ends or a
/// seek signal arrives.
fn pump(
    appsrc: &gst_app::AppSrc,
    rx: &Receiver<FeedSignal>,
    stream: &mut dyn Read,
) -> Result<FeedOutcome, VideoStreamError> {
    let mut buffer = vec![0u8; CHUNK_SIZE];
    loop {
        // Serve a pending seek signal: acknowledge it (the `seek_data`
        // callback is blocked waiting for that) and continue with the run at
        // the requested offset.
        let mut seek = None;
        while let Ok(signal) = rx.try_recv() {
            let FeedSignal::Seek(offset, ack) = signal;
            if ack.send(()).is_err() {
                // The `seek_data` callback gave up: the pipeline is gone.
                return Err(VideoStreamError::Io(
                    "seek was abandoned while repositioning the stream".into(),
                ));
            }
            seek = Some(offset);
        }
        if let Some(offset) = seek {
            return Ok(FeedOutcome::SeekAt(offset));
        }

        // Never block inside `push_buffer` waiting for the queue to drain:
        // the demuxer might at that moment be blocked inside `seek_data`,
        // waiting for THIS thread to acknowledge a seek -- which a blocked
        // push can never service, deadlocking the pipeline (e.g. when
        // qtdemux parses the `moov` at the end of a non-faststart file
        // right after reading its head). Wait for the queue to drain
        // instead, keeping the seek channel serviced so `seek_data` is
        // always answered.
        while appsrc.current_level_bytes() >= MAX_QUEUE_BYTES - CHUNK_SIZE as u64 {
            match rx.recv_timeout(std::time::Duration::from_millis(10)) {
                Ok(FeedSignal::Seek(offset, ack)) => {
                    if ack.send(()).is_err() {
                        return Err(VideoStreamError::Io(
                            "seek was abandoned while repositioning the stream".into(),
                        ));
                    }
                    return Ok(FeedOutcome::SeekAt(offset));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(VideoStreamError::Io(
                        "seek channel disconnected while the queue was full".into(),
                    ));
                }
            }
        }

        let n = stream.read(&mut buffer)?;
        if n == 0 {
            appsrc
                .end_of_stream()
                .map_err(|e| VideoStreamError::Io(format!("failed to end stream: {e:?}")))?;
            return Ok(FeedOutcome::Eos);
        }

        // Push the chunk, retrying while the pipeline isn't started yet:
        // appsrc rejects pushes outside Paused/Playing, and the pipeline is
        // still in Null/Ready when this thread starts feeding it.
        loop {
            match appsrc.push_buffer(gst::Buffer::from_slice(buffer[..n].to_vec())) {
                Ok(_) => break,
                // The queue was flushed: either a seek is in flight (jump to
                // the signal handling) or the pipeline has failed/was stopped
                // below Paused -- in the latter case pushing can never
                // succeed again, so stop feeding.
                Err(gst::FlowError::Flushing) => match appsrc.current_state() {
                    gst::State::Null | gst::State::Ready => {
                        return Err(VideoStreamError::Io(
                            "pipeline stopped before the video finished playing".into(),
                        ));
                    }
                    _ => {
                        break;
                    }
                },
                Err(e) => match appsrc.current_state() {
                    gst::State::Null | gst::State::Ready => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    _ => {
                        return Err(VideoStreamError::Io(format!("push failed: {e:?}")));
                    }
                },
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::let_underscore_must_use)]
mod tests {
    use super::*;
    use std::{path::PathBuf, process::Command, time::Duration};

    fn make_test_file(name: &str, faststart: bool, with_audio: bool) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=320x240:rate=30",
        ]);
        if with_audio {
            cmd.args(["-f", "lavfi", "-i", "sine=duration=2", "-c:a", "aac"]);
        }
        cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
        if faststart {
            cmd.args(["-movflags", "+faststart"]);
        }
        cmd.arg(&path);
        cmd.status()
            .expect("ffmpeg should be installed for tests")
            .success()
            .then_some(())
            .expect("ffmpeg should succeed");

        path
    }

    /// Like [`make_test_file`], but large enough that the appsrc queue fills
    /// while it is being fed: bigger than [`MAX_QUEUE_BYTES`], non-faststart.
    fn make_large_test_file(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=10:size=1280x720:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=10",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            // Force the file past MAX_QUEUE_BYTES so feeding fills the queue.
            "-b:v",
            "9M",
            "-c:a",
            "aac",
        ]);
        cmd.arg(&path);
        cmd.status()
            .expect("ffmpeg should be installed for tests")
            .success()
            .then_some(())
            .expect("ffmpeg should succeed");

        path
    }

    /// Whether the video is really decoding: position (queried through the
    /// appsink, since `Video::position` needs a playbin pipeline) advances.
    fn plays(video: &Video) -> bool {
        let appsink = video
            .pipeline()
            .by_name("iced_video")
            .expect("video appsink should exist");

        let mut last_position = gst::ClockTime::ZERO;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(300));

            let position = appsink
                .query_position::<gst::ClockTime>()
                .unwrap_or(gst::ClockTime::ZERO);
            assert!(position >= last_position, "position should advance");
            last_position = position;
            if position > gst::ClockTime::from_mseconds(500) {
                return true;
            }
        }
        false
    }

    #[test]
    fn streams_plain_file() {
        let path = make_test_file("deplace-stream-test-plain.mp4", true, true);

        let (video, _feeder) = open_stream(&path, None, "autoaudiosink sync=true")
            .expect("streaming video should start");

        assert!(video.size().0 > 0);
        assert!(plays(&video), "video should have started playing");

        video.set_paused(true);
    }

    #[test]
    fn streams_plain_file_with_seekable_container() {
        // `moov` at the end: the demuxer must seek while opening the file.
        let path = make_test_file("deplace-stream-test-end.mp4", false, true);

        let (video, _feeder) = open_stream(&path, None, "autoaudiosink sync=true")
            .expect("streaming video should start");

        assert!(video.size().0 > 0);
        assert!(plays(&video), "video should have started playing");

        video.set_paused(true);
    }

    #[test]
    fn streams_large_file_larger_than_the_queue() {
        // Regression test: a non-faststart file bigger than the appsrc queue.
        // The feeder fills the queue faster than the demuxer spins up, so the
        // `moov` seek arrives while the queue is full -- the feeder must stay
        // responsive to it instead of blocking inside `push_buffer`, or the
        // pipeline deadlocks and opening never finishes.
        let path = make_large_test_file("deplace-stream-test-large.mp4");

        // Watchdog: with the deadlock this never returned at all.
        let (tx, rx) = std::sync::mpsc::channel();
        let open_path = path.clone();
        std::thread::spawn(move || {
            let opened = open_stream(&open_path, None, "autoaudiosink sync=true");
            let _ = tx.send(opened);
        });
        let opened = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("opening a large non-faststart file must not get stuck");

        let (video, _feeder) = opened.expect("streaming video should start");
        assert!(video.size().0 > 0);
        assert!(plays(&video), "video should have started playing");

        video.set_paused(true);
    }

    #[test]
    fn plays_and_seeks_without_audio_stream() {
        // Files without an audio or text stream leave those sink chains
        // unlinked: they must not block the pipeline, and seeks (looping)
        // must keep working.
        let path = make_test_file("deplace-stream-test-no-audio.mp4", false, false);

        let (video, _feeder) = open_stream(&path, None, "autoaudiosink sync=true")
            .expect("video without audio should start");
        assert!(plays(&video), "video without audio should play");

        std::thread::sleep(Duration::from_millis(3500));

        video
            .restart_stream()
            .expect("restart after eos should succeed");
        assert!(
            plays(&video),
            "video without audio should restart after EOS"
        );

        video.set_paused(true);
    }

    #[test]
    fn restarts_after_eos() {
        // Reproduces what looping does internally: after the stream ends, the
        // player re-seeks to the start and the parked feeder has to pick the
        // stream back up.
        let path = make_test_file("deplace-stream-test-loop.mp4", false, true);

        let (video, _feeder) = open_stream(&path, None, "autoaudiosink sync=true")
            .expect("streaming video should start");
        assert!(plays(&video), "video should have started playing");

        // Wait for the stream to run out (the test videos are 2 seconds), plus
        // a grace period: the restart seek can race the tail of the EOS
        // handling, so give it a retry before declaring failure.
        std::thread::sleep(Duration::from_millis(4000));

        let mut restart = video.restart_stream();
        if restart.is_err() {
            std::thread::sleep(Duration::from_millis(1000));
            restart = video.restart_stream();
        }
        restart.expect("restart after eos should succeed");
        assert!(plays(&video), "video should restart after EOS");

        video.set_paused(true);
    }

    #[test]
    fn streams_locally_encrypted_file() {
        // Plain downloads are cached encrypted by [`AttachmentWriter`]; this
        // plays such a file the same way the video cache does.
        use matrix_sdk_base::crypto::MediaEncryptionInfo;
        use std::io::Write;

        use crate::state::local_file_encryption::AttachmentWriter;

        let plain_path = make_test_file("deplace-stream-test-local-enc.mp4", false, true);
        let plain_bytes = std::fs::read(&plain_path).unwrap();
        let encrypted_path = std::env::temp_dir().join("deplace-stream-test-local-enc.bin");

        let mut writer =
            AttachmentWriter::new(std::fs::File::create(&encrypted_path).unwrap()).unwrap();

        // Network chunks arrive in irregular sizes; feed the writer the same
        // distribution the download loop would.
        let mut write_sizes =
            std::collections::VecDeque::from([8192usize, 12288, 16384, 901, 16384, 8192, 319]);
        let mut rest = plain_bytes.as_slice();
        while !rest.is_empty() {
            let size = write_sizes.pop_front().unwrap_or(16384);
            if write_sizes.is_empty() {
                write_sizes.extend([8192usize, 12288, 16384, 901, 16384, 8192, 319]);
            }
            let take = rest.len().min(size);
            let (chunk, remaining) = rest.split_at(take);
            writer.write_all(chunk).unwrap();
            rest = remaining;
        }
        let info: MediaEncryptionInfo = writer.finish().unwrap();

        let url = ruma::owned_mxc_uri!("mxc://localhost/deplace-stream-test-local-enc");
        let encrypted_file =
            ruma::events::room::EncryptedFile::new(url, info.encryption_info, info.hashes);

        let (video, _feeder) = open_stream(&encrypted_path, Some(&encrypted_file), "fakesink")
            .expect("streaming locally encrypted video should start");

        assert!(video.size().0 > 0);
        assert!(plays(&video), "locally encrypted video should play");

        video.set_paused(true);
    }

    #[test]
    fn streams_larger_locally_encrypted_file() {
        // A multi-megabyte file with `moov` at the end reproduces the real
        // download flow: the feeder pushes many chunks, then qtdemux decides
        // to seek past the (large) mdat box once it is already being fed,
        // forcing the feeder to rebuild mid-stream. The small test files fit
        // into a single feed chunk and never exercise this.
        use matrix_sdk_base::crypto::MediaEncryptionInfo;
        use std::io::{Read as _, Write};

        use crate::state::local_file_encryption::AttachmentWriter;

        let plain_path = std::env::temp_dir().join("deplace-stream-test-big.mp4");
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=duration=6:size=1280x720:rate=30",
            "-c:v",
            "libx264",
            "-b:v",
            "8M",
            "-pix_fmt",
            "yuv420p",
        ]);
        cmd.arg(&plain_path);
        cmd.status()
            .expect("ffmpeg should be installed for tests")
            .success()
            .then_some(())
            .expect("ffmpeg should succeed");

        // The container really has its `moov` at the end and a large mdat:
        // the head must parse as ftyp + mdat.
        let mut head = [0u8; 8];
        std::fs::File::open(&plain_path)
            .and_then(|mut file| file.read_exact(&mut head))
            .unwrap();
        assert_eq!(&head[4..], b"ftyp", "test file must be an mp4");

        let plain_len = std::fs::metadata(&plain_path).unwrap().len();
        assert!(
            plain_len > 1024 * 1024,
            "test file must span multiple feed chunks"
        );

        let encrypted_path = std::env::temp_dir().join("deplace-stream-test-big.bin");

        let mut writer =
            AttachmentWriter::new(std::fs::File::create(&encrypted_path).unwrap()).unwrap();
        // Feed realistically sized chunks like the download loop does.
        let file = std::fs::File::open(&plain_path).unwrap();
        let mut reader = std::io::BufReader::new(file);
        let mut buffer = [0u8; 16384];
        loop {
            let n = reader.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            writer.write_all(&buffer[..n]).unwrap();
        }
        let info: MediaEncryptionInfo = writer.finish().unwrap();

        let url = ruma::owned_mxc_uri!("mxc://localhost/deplace-stream-test-big");
        let encrypted_file =
            ruma::events::room::EncryptedFile::new(url, info.encryption_info, info.hashes);

        let (video, _feeder) = open_stream(&encrypted_path, Some(&encrypted_file), "fakesink")
            .expect("streaming larger locally encrypted video should start");
        assert!(plays(&video), "larger locally encrypted video should play");

        // The moov-at-end file requires the appsrc to be seekable: playing
        // would be impossible without the mid-stream rebuild working.
        video.set_paused(true);
    }

    #[test]
    fn streams_encrypted_file() {
        use matrix_sdk_base::crypto::AttachmentEncryptor;

        let plain_path = make_test_file("deplace-stream-test-enc-src.mp4", false, true);
        let encrypted_path = std::env::temp_dir().join("deplace-stream-test-enc.mp4");
        let mut plain = std::io::BufReader::new(std::fs::File::open(&plain_path).unwrap());
        let mut encrypted =
            std::fs::File::create(&encrypted_path).expect("creating encrypted test file");

        let mut encryptor = AttachmentEncryptor::new(&mut plain);
        std::io::copy(&mut encryptor, &mut encrypted).unwrap();
        let info = encryptor.finish();

        let url = ruma::owned_mxc_uri!("mxc://localhost/deplace-stream-test");
        let encrypted_file =
            ruma::events::room::EncryptedFile::new(url, info.encryption_info, info.hashes);

        let (video, _feeder) = open_stream(&encrypted_path, Some(&encrypted_file), "fakesink")
            .expect("streaming encrypted video should start");

        assert!(video.size().0 > 0);
        assert!(plays(&video), "encrypted video should have started playing");

        video.set_paused(true);
    }

    #[test]
    fn opens_huge_moov_at_end_encrypted_file_quickly() {
        // Reproduces the real-world failure: a 100MB+, non-faststart mp4.
        // The demuxer seeks to the far-end `moov` box on open; the seek must
        // be an O(1) counter jump (not a decryptor fast-forward through the
        // whole file), and the player must wait for the resulting caps
        // instead of giving up after 5 seconds.
        use matrix_sdk_base::crypto::{AttachmentEncryptor, MediaEncryptionInfo};
        use std::io::Write as _;

        let plain_path = std::env::temp_dir().join("deplace-stream-test-huge.mp4");
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "mandelbrot=size=1280x720:rate=30",
            "-t",
            "10",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "300M",
        ]);
        cmd.arg(&plain_path);
        cmd.status()
            .expect("ffmpeg should be installed for tests")
            .success()
            .then_some(())
            .expect("ffmpeg should succeed");

        // The head must parse as ftyp + mdat (moov at the very end).
        let mut head = [0u8; 8];
        std::fs::File::open(&plain_path)
            .and_then(|mut file| file.read_exact(&mut head))
            .unwrap();
        assert_eq!(&head[4..], b"ftyp", "test file must be an mp4");
        let plain_len = std::fs::metadata(&plain_path).unwrap().len();
        assert!(
            plain_len > 100 * 1024 * 1024,
            "test file must be huge, got {plain_len} bytes"
        );

        let encrypted_path = std::env::temp_dir().join("deplace-stream-test-huge.bin");
        let mut writer = std::io::BufWriter::new(
            std::fs::File::create(&encrypted_path).expect("creating encrypted test file"),
        );
        let mut plain_reader = std::io::BufReader::new(std::fs::File::open(&plain_path).unwrap());
        let mut encryptor = AttachmentEncryptor::new(&mut plain_reader);
        std::io::copy(&mut encryptor, &mut writer).unwrap();
        let info: MediaEncryptionInfo = encryptor.finish();
        writer.flush().unwrap();
        drop(writer);

        let url = ruma::owned_mxc_uri!("mxc://localhost/deplace-stream-test-huge");
        let encrypted_file =
            ruma::events::room::EncryptedFile::new(url, info.encryption_info, info.hashes);

        let started = std::time::Instant::now();
        let opened = open_stream(&encrypted_path, Some(&encrypted_file), "fakesink");
        let (video, _feeder) = opened.expect("streaming encrypted video should start");
        let elapsed = started.elapsed();
        println!("opened huge moov-at-end video in {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "opening must not need to decrypt the whole file"
        );

        assert!(video.size().0 > 0);
        assert!(plays(&video), "huge encrypted video should be playing");

        video.set_paused(true);
    }
}
