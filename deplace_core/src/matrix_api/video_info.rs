//! Probes a video by decoding a single frame to a JPEG with GStreamer.

use gstreamer::prelude::*;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::time::Duration;

/// Metadata probed from a video file.
pub struct VideoMetadata {
    /// The width of the video in pixels.
    pub width: Option<u32>,
    /// The height of the video in pixels.
    pub height: Option<u32>,
    /// The duration of the video.
    pub duration: Option<Duration>,
    /// The blurhash of a representative frame.
    pub blurhash: Option<String>,
    /// A representative frame encoded as a JPEG thumbnail.
    pub thumbnail_jpeg: Option<Vec<u8>>,
    /// Width of `thumbnail_jpeg` in pixels, if the frame decoded.
    pub thumbnail_width: Option<u32>,
    /// Height of `thumbnail_jpeg` in pixels, if the frame decoded.
    pub thumbnail_height: Option<u32>,
}

/// Maximum width of the generated thumbnail.
const THUMBNAIL_MAX_WIDTH: u32 = 800;

/// The pipeline stops itself after the first decoded frame.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Raises the rank of VA-API hardware decoders above the software ones so
/// `decodebin`/`uridecodebin` plug them automatically.
pub(crate) fn prefer_hardware_decoders() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for name in [
            "vah264dec",
            "vah265dec",
            "vaav1dec",
            "vampeg2dec",
            "vajpegdec",
            "vavp8dec",
            "vavp9dec",
        ] {
            if let Some(factory) = gstreamer::ElementFactory::find(name) {
                // Above avdec/software decoders (rank PRIMARY = 256).
                factory.set_rank(gstreamer::Rank::from(512));
            }
        }
    });
}

/// Probes one decoded frame for width, height, duration and blurhash, on
/// failure the upload still proceeds with less metadata.
#[allow(clippy::let_underscore_must_use)]
pub fn probe_video(data: &[u8], file_suffix: &str) -> VideoMetadata {
    let mut metadata = VideoMetadata {
        width: None,
        height: None,
        duration: None,
        blurhash: None,
        thumbnail_jpeg: None,
        thumbnail_width: None,
        thumbnail_height: None,
    };

    let Some(temp_path) = write_temp_file(data, file_suffix) else {
        return metadata;
    };

    if let Err(error) = gstreamer::init() {
        tracing::warn!("GStreamer init failed: {error}");
        let _ = std::fs::remove_file(&temp_path);
        return metadata;
    }
    prefer_hardware_decoders();

    let Ok(uri) = gstreamer::glib::filename_to_uri(&temp_path, None) else {
        tracing::warn!("Failed to build URI from video file");
        let _ = std::fs::remove_file(&temp_path);
        return metadata;
    };

    let pipeline_description =
        format!("uridecodebin uri={uri} ! videoconvert ! jpegenc ! appsink name=sink");

    let probed = match gstreamer::parse::launch(&pipeline_description)
        .ok()
        .and_then(|element| element.downcast::<gstreamer::Pipeline>().ok())
    {
        Some(pipeline) => {
            let sink = pipeline
                .by_name("sink")
                .and_then(|element| element.downcast::<gstreamer_app::AppSink>().ok());

            // Preroll first so the pipeline can negotiate caps, then pull
            // exactly one encoded JPEG frame and stop
            let _ = pipeline.set_state(gstreamer::State::Paused);
            if let Some(bus) = pipeline.bus() {
                let _ = bus.timed_pop_filtered(
                    gstreamer::ClockTime::from_nseconds(PROBE_TIMEOUT.as_nanos() as u64),
                    &[
                        gstreamer::MessageType::StateChanged,
                        gstreamer::MessageType::Error,
                    ],
                );
            }

            let sample = sink.and_then(|sink| {
                let _ = pipeline.set_state(gstreamer::State::Playing);
                sink.try_pull_sample(gstreamer::ClockTime::from_seconds(PROBE_TIMEOUT.as_secs()))
            });
            // The duration is only reliable once the pipeline has prerolled
            // and played at least one buffer.
            let duration = pipeline.query_duration::<gstreamer::ClockTime>();
            let _ = pipeline.set_state(gstreamer::State::Null);

            match sample {
                Some(sample) => {
                    let jpeg = sample
                        .buffer()
                        .and_then(|buffer| buffer.map_readable().ok())
                        .map(|bytes| bytes.as_slice().to_vec());
                    let (width, height) = sample
                        .caps()
                        .and_then(|caps| gstreamer_video::VideoInfo::from_caps(&caps).ok())
                        .map(|info| (Some(info.width()), Some(info.height())))
                        .unwrap_or((None, None));
                    (duration, jpeg, width, height)
                }
                None => {
                    tracing::warn!("Video probing produced no frame");
                    (duration, None, None, None)
                }
            }
        }
        None => {
            tracing::warn!("Failed to build video probing pipeline");
            (None, None, None, None)
        }
    };
    let (duration, jpeg, width, height) = probed;
    if let Some(duration) = duration {
        metadata.duration = Some(Duration::from_nanos(duration.nseconds()));
    }
    if width.is_some() {
        metadata.width = width;
        metadata.height = height;
    }

    let _ = std::fs::remove_file(&temp_path);

    let thumbnail = match jpeg.and_then(|jpeg| {
        image::ImageReader::new(Cursor::new(jpeg))
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()
    }) {
        Some(img) => {
            if metadata.width.is_none() {
                metadata.width = Some(img.width());
                metadata.height = Some(img.height());
            }
            let thumb = img.thumbnail(64, 64);
            metadata.blurhash =
                blurhash::encode(4, 3, thumb.width(), thumb.height(), &thumb.to_rgba8()).ok();

            let scaled = if img.width() > THUMBNAIL_MAX_WIDTH {
                img.thumbnail(THUMBNAIL_MAX_WIDTH, u32::MAX)
            } else {
                img
            };
            metadata.thumbnail_width = Some(scaled.width());
            metadata.thumbnail_height = Some(scaled.height());
            let mut out = std::io::Cursor::new(Vec::new());
            image::codecs::jpeg::JpegEncoder::new(&mut out)
                .encode_image(&scaled)
                .ok()
                .map(|_| out.into_inner())
        }
        None => {
            tracing::warn!("Failed to decode JPEG frame for video thumbnail");
            None
        }
    };

    metadata.thumbnail_jpeg = thumbnail;
    metadata
}

/// Writes the bytes to a uniquely named temporary file, returning `None` and
/// logging on failure.
#[allow(clippy::let_underscore_must_use)]
fn write_temp_file(data: &[u8], file_suffix: &str) -> Option<PathBuf> {
    let mut path = std::env::temp_dir().join(format!(
        "deplace-video-{}{file_suffix}",
        uuid::Uuid::new_v4()
    ));
    if file_suffix.is_empty() {
        path.set_extension("mp4");
    }

    let Ok(mut file) = std::fs::File::create(&path) else {
        tracing::warn!("Failed to create temp file for video probing");
        return None;
    };
    if file.write_all(data).is_err() {
        tracing::warn!("Failed to write temp file for video probing");
        let _ = std::fs::remove_file(&path);
        return None;
    }

    Some(path)
}
