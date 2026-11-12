// Reproduces DePlace's appsrc streaming pipeline (from video_stream.rs)
// against a local (plaintext) file: appsrc (seekable, blocking, 8MB queue)
// + feeder thread + decodebin + sink chains + Video::from_gst_pipeline.
//
// The feeder must stay responsive to `seek_data` while the queue is full:
// a blocked push can never acknowledge the demuxer's `moov`-at-the-end
// seek, which deadlocks the pipeline. Feeding a non-faststart file larger
// than the queue exercises exactly that; without the drain-wait poll this
// wedges reliably on such files.
//
// Usage: appsrc_repro <file.mp4>
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use iced_video_player::Video;

use std::{
    borrow::Cow,
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
};

const MAX_QUEUE_BYTES: u64 = 8 * 1024 * 1024;
const CHUNK_SIZE: usize = 512 * 1024;

enum FeedSignal {
    Seek(u64, std::sync::mpsc::Sender<()>),
}

enum Outcome {
    SeekAt(u64),
    Eos,
}

fn main() {
    let path: PathBuf = std::env::args()
        .nth(1)
        .expect("usage: appsrc_repro <file.mp4>")
        .into();

    gst::init().unwrap();
    let file_size = std::fs::metadata(&path).unwrap().len();

    let pipeline = gst::Pipeline::with_name("deplace-stream");
    let appsrc = gst::ElementFactory::make("appsrc")
        .name("videosrc")
        .build()
        .unwrap()
        .downcast::<gst_app::AppSrc>()
        .unwrap();
    appsrc.set_format(gst::Format::Bytes);
    appsrc.set_stream_type(gst_app::AppStreamType::Seekable);
    appsrc.set_block(true);
    appsrc.set_max_bytes(MAX_QUEUE_BYTES);
    appsrc.set_size(file_size as i64);

    let decodebin = gst::ElementFactory::make("decodebin").name("decoder").build().unwrap();

    let video_bin = gst::parse::bin_from_description(
        "queue ! videoscale ! videoconvert ! appsink name=iced_video drop=true sync=true caps=video/x-raw,format=NV12,pixel-aspect-ratio=1/1",
        true,
    )
    .unwrap();
    let audio_bin =
        gst::parse::bin_from_description("queue ! audioconvert ! audioresample ! autoaudiosink sync=true", true).unwrap();

    pipeline
        .add_many([
            appsrc.upcast_ref::<gst::Element>(),
            decodebin.upcast_ref::<gst::Element>(),
            video_bin.upcast_ref::<gst::Element>(),
            audio_bin.upcast_ref::<gst::Element>(),
        ])
        .unwrap();
    appsrc.link(decodebin.upcast_ref::<gst::Element>()).unwrap();

    {
        let video_bin = video_bin.clone();
        let audio_bin = audio_bin.clone();
        decodebin.connect_pad_added(move |_, pad| {
            let Some(caps) = pad.current_caps() else { return };
            let Some(s) = caps.structure(0) else { return };
            if s.name().starts_with("video") {
                if let Some(target) = video_bin.static_pad("sink") {
                    let _ = pad.link(&target);
                }
            } else if s.name().starts_with("audio") {
                if let Some(target) = audio_bin.static_pad("sink") {
                    let _ = pad.link(&target);
                }
            }
        });
    }

    let (tx, rx) = channel::<FeedSignal>();
    let callback_tx = tx.clone();
    appsrc.set_callbacks(
        gst_app::AppSrcCallbacks::builder()
            .seek_data(move |_, offset| {
                eprintln!("[seek_data] offset={offset}, waiting for feeder ack");
                let (ack_tx, ack_rx) = channel::<()>();
                let ok =
                    callback_tx.send(FeedSignal::Seek(offset, ack_tx)).is_ok()
                        && ack_rx.recv().is_ok();
                eprintln!("[seek_data] offset={offset} acked={ok}");
                ok
            })
            .build(),
    );

    let feed_appsrc = appsrc.clone();
    std::thread::Builder::new()
        .name("deplace-video-stream".into())
        .spawn(move || feed(&feed_appsrc, &rx, &path))
        .unwrap();

    let video_appsink = video_bin
        .by_name("iced_video")
        .and_downcast::<gst_app::AppSink>()
        .unwrap();

    let start = std::time::Instant::now();
    match Video::from_gst_pipeline(pipeline, video_appsink, None) {
        Ok(video) => println!("OK in {:?}: size {:?}", start.elapsed(), video.size()),
        Err(e) => {
            println!("FAILED in {:?}: {e}", start.elapsed());
            std::process::exit(1);
        }
    }
}

fn feed(appsrc: &gst_app::AppSrc, rx: &Receiver<FeedSignal>, path: &Path) {
    let mut offset = 0u64;
    loop {
        match stream_run(appsrc, rx, path, offset) {
            Ok(Outcome::SeekAt(seek)) => offset = seek,
            Ok(Outcome::Eos) => match rx.recv() {
                Ok(FeedSignal::Seek(seek, ack)) => {
                    if ack.send(()).is_err() {
                        return;
                    }
                    offset = seek;
                }
                Err(_) => return,
            },
            Err(e) => {
                eprintln!("feeder failed: {e}");
                return;
            }
        }
    }
}

fn stream_run(
    appsrc: &gst_app::AppSrc,
    rx: &Receiver<FeedSignal>,
    path: &Path,
    offset: u64,
) -> Result<Outcome, Cow<'static, str>> {
    let mut plain = std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
    pump(appsrc, rx, &mut plain, offset)
}

fn pump(
    appsrc: &gst_app::AppSrc,
    rx: &Receiver<FeedSignal>,
    stream: &mut dyn Read,
    offset: u64,
) -> Result<Outcome, Cow<'static, str>> {
    let mut remaining = offset;
    let mut scratch = vec![0u8; CHUNK_SIZE];
    while remaining > 0 {
        let n = stream
            .read(&mut scratch[..(remaining.min(CHUNK_SIZE as u64)) as usize])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("unexpected end".into());
        }
        remaining -= n as u64;
    }

    let mut buffer = vec![0u8; CHUNK_SIZE];
    loop {
        // Serve seek signals first, at least whenever starting a new chunk.
        let mut seek = None;
        while let Ok(signal) = rx.try_recv() {
            let FeedSignal::Seek(offset, ack) = signal;
            ack.send(()).map_err(|_| "seek was abandoned".to_string())?;
            seek = Some(offset);
        }
        if let Some(offset) = seek {
            return Ok(Outcome::SeekAt(offset));
        }

        // FIX (mirrors deplace's video_stream.rs): never block inside
        // push_buffer on a full queue -- the consumer may be blocked inside
        // seek_data waiting for THIS thread to ack a seek. Drain-wait with
        // a poll that stays responsive to seeks instead.
        while appsrc.current_level_bytes() >= MAX_QUEUE_BYTES - CHUNK_SIZE as u64 {
            match rx.recv_timeout(std::time::Duration::from_millis(10)) {
                Ok(FeedSignal::Seek(offset, ack)) => {
                    if ack.send(()).is_err() {
                        return Err("seek was abandoned while repositioning".into());
                    }
                    eprintln!("[feed] seek to {offset} served from drain-wait");
                    return Ok(Outcome::SeekAt(offset));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("disconnected".into())
                }
            }
        }

        let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            appsrc.end_of_stream().map_err(|e| e.to_string())?;
            return Ok(Outcome::Eos);
        }

        // Push, retrying while the pipeline isn't started yet.
        loop {
            match appsrc.push_buffer(gst::Buffer::from_slice(buffer[..n].to_vec())) {
                Ok(_) => break,
                Err(gst::FlowError::Flushing) => match appsrc.current_state() {
                    gst::State::Null | gst::State::Ready => {
                        return Err("pipeline stopped".into());
                    }
                    _ => break,
                },
                Err(_) => match appsrc.current_state() {
                    gst::State::Null | gst::State::Ready => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    _ => return Err("push failed".into()),
                }
            }
        }
    }
}
