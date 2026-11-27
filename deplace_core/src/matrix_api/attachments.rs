use std::{
    io::Cursor,
    path::PathBuf,
    sync::Arc,
    time::SystemTime,
};

use anyhow::Result;
use bytes::Bytes;
use matrix_sdk::media::MediaRequestParameters;
use rfd::{AsyncFileDialog, FileHandle};

use mime::Mime;
use ruma::events::room::MediaSource;
use tokio::{fs::File, io::AsyncWriteExt};

use crate::{
    formatting::format_bytes,
    settings::DataSizeUnit,
    state::{
        AppState,
        media_fetcher::{CompressionProgress, MediaProcessor},
    },
};

#[derive(Clone, PartialEq, Eq)]
pub struct MatrixAttachment {
    pub filename: String,
    pub mime_type: Mime,
    pub data: Bytes,
    /// Where the bytes came from, so probing can decode the file itself instead
    /// of a second copy of it.
    pub source_path: Option<PathBuf>,
}

impl std::fmt::Debug for MatrixAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixAttachment")
            .field("filename", &self.filename)
            .field("mime_type", &self.mime_type)
            .field("data", &format!("<{} bytes>", self.data.len()))
            .finish()
    }
}

impl MatrixAttachment {
    pub fn from_bytes(filename: &str, mime_type: Mime, data: impl Into<Bytes>) -> Self {
        Self {
            filename: filename.to_string(),
            mime_type,
            data: data.into(),
            source_path: None,
        }
    }

    /// Builds a PNG attachment from raw RGBA pixels, e.g. an image copied to the clipboard.
    pub fn from_rgba(filename: &str, width: u32, height: u32, rgba: &[u8]) -> Result<Self> {
        let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| anyhow::anyhow!("Pixel buffer does not match the given dimensions"))?;

        let mut data = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut data), image::ImageFormat::Png)?;

        Ok(Self::from_bytes(filename, mime::IMAGE_PNG, data))
    }

    async fn from_file_handle(handle: FileHandle) -> Result<Self> {
        let path = handle.path().to_path_buf();
        if path.is_dir() {
            tracing::warn!("{} is a directory, not a file", path.display());
            anyhow::bail!("{} is a directory, not a file", path.display());
        }
        let read_start = std::time::Instant::now();
        check_upload_fits(&path)?;
        let data = handle.read().await;
        tracing::info!(
            "Read {} ({} bytes) in {:?}",
            path.display(),
            data.len(),
            read_start.elapsed()
        );
        let mime_type = mime_guess::from_path(&path).first_or_octet_stream();
        Ok(Self {
            filename: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string(),
            mime_type,
            data: data.into(),
            source_path: Some(path),
        })
    }

    /// Builds an attachment from an arbitrary path, e.g. a dropped file.
    /// Directories are zipped the same way `compressed_from_folder_picker` does.
    pub async fn from_path(path: std::path::PathBuf, state: AppState) -> Result<Self> {
        if path.is_dir() {
            return Self::compress_folder(path, state).await;
        }
        check_upload_fits(&path)?;
        let data = tokio::fs::read(&path).await?;
        let mime_type = mime_guess::from_path(&path).first_or_octet_stream();
        Ok(Self {
            filename: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string(),
            mime_type,
            data: data.into(),
            source_path: Some(path),
        })
    }

    pub async fn from_files_picker() -> Vec<MatrixAttachment> {
        let Some(file_handles) = AsyncFileDialog::new()
            .set_title("Select files")
            .pick_files()
            .await
        else {
            tracing::warn!("No files selected");
            return Vec::new();
        };

        let mut array = Vec::new();

        for handle in file_handles {
            match MatrixAttachment::from_file_handle(handle).await {
                Ok(attachment) => array.push(attachment),
                Err(e) => {
                    tracing::error!("Failed to attach file: {}", e);
                }
            }
        }

        array
    }

    /// Attaches every `file://` URI in the given clipboard text
    /// Text without file URIs yields no attachments.
    pub async fn from_clipboard_file_list(text: String) -> Vec<MatrixAttachment> {
        // Each line is either a `file://` URI or a plain
        // filesystem path
        let paths: Vec<std::path::PathBuf> = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .filter_map(|line| match url::Url::parse(line) {
                Ok(url) if url.scheme() == "file" => url.to_file_path().ok(),
                Ok(_) => {
                    tracing::debug!("Skipping non-file clipboard URI: {line}");
                    None
                }
                Err(_) => Some(std::path::PathBuf::from(line)),
            })
            .collect();
        if paths.is_empty() {
            tracing::debug!("No usable clipboard file paths/URIs in: {text:?}");
        }

        let mut array = Vec::new();
        for path in paths {
            let data = match tokio::fs::read(&path).await {
                Ok(data) => data,
                Err(e) => {
                    tracing::error!("Failed to read clipboard file {}: {}", path.display(), e);
                    continue;
                }
            };
            let mime_type = mime_guess::from_path(&path).first_or_octet_stream();
            array.push(Self::from_bytes(
                path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
                mime_type,
                data,
            ));
        }

        array
    }

    pub async fn compressed_from_folder_picker(state: AppState) -> Option<MatrixAttachment> {
        let Some(folder_handle) = AsyncFileDialog::new()
            .set_title("Select folder to compress")
            .pick_folder()
            .await
        else {
            tracing::warn!("No folder selected");
            return None;
        };

        match Self::compress_folder(folder_handle.path().to_path_buf(), state).await {
            Ok(attachment) => Some(attachment),
            Err(e) => {
                tracing::error!("Failed to compress folder: {}", e);
                None
            }
        }
    }

    async fn compress_folder(folder_path: std::path::PathBuf, state: AppState) -> Result<Self> {
        let folder_name = folder_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder")
            .to_string();

        let total: u64 = walkdir::WalkDir::new(folder_path.clone())
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();

        let media_processor = state.media_processor();
        let progress = media_processor.start_compression(folder_path.clone(), total);
        let started_at = progress.start_time();

        let path_for_blocking = folder_path.clone();
        let media_processor_for_blocking = media_processor.clone();

        let data = match tokio::task::spawn_blocking(move || {
            zip_folder(&path_for_blocking, &progress, &media_processor_for_blocking)
        })
        .await
        {
            Ok(Ok(data)) => {
                media_processor.complete_compression(folder_path, started_at);
                data
            }
            Ok(Err(e)) => {
                media_processor.fail_compression(folder_path, started_at, Arc::new(e.to_string()));
                return Err(e);
            }
            Err(e) => {
                let msg = e.to_string();
                media_processor.fail_compression(folder_path, started_at, Arc::new(msg.clone()));
                anyhow::bail!("Compression task failed: {msg}");
            }
        };

        tracing::info!(
            "Compressed {} ({} bytes) in {:?}",
            folder_name,
            data.len(),
            started_at.elapsed()
        );

        Ok(Self::from_bytes(
            &format!("{folder_name}.zip"),
            "application/zip".parse().expect("valid mime type"),
            data,
        ))
    }
}

fn zip_folder(
    folder_path: &std::path::Path,
    progress: &CompressionProgress,
    media_processor: &MediaProcessor,
) -> Result<Vec<u8>> {
    use std::io::Write;

    let mut buffer = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in walkdir::WalkDir::new(folder_path) {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(folder_path)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let name = relative.to_string_lossy();

        if path.is_dir() {
            writer.add_directory(name, options)?;
        } else if path.is_file() {
            let data = std::fs::read(path)?;
            writer.start_file(name, options)?;
            writer.write_all(&data)?;

            progress.add_to_compressed_bytes(data.len() as u64);
            media_processor.notify_progress();
        }
    }

    writer.finish()?;
    Ok(buffer.into_inner())
}

pub async fn download_file_to_picked(source: MediaSource, file_name: String, state: AppState) {
    let Some(handle) = AsyncFileDialog::new()
        .set_title("File destination")
        .set_file_name(file_name)
        .save_file()
        .await
    else {
        tracing::error!("No path to save file selected");
        return;
    };

    let path = handle.path();

    let request = MediaRequestParameters {
        source,
        format: matrix_sdk::media::MediaFormat::File,
    };
    let media_processor = state.media_processor();
    media_processor.add_relevant_source(&request, path.into());

    let started_at = SystemTime::now();
    let content = match state
        .client()
        .media()
        .get_media_content(&request, true)
        .await
    {
        Ok(content) => {
            media_processor.ensure_download_recorded(&request, started_at, &Ok(()));
            content
        }
        Err(e) => {
            tracing::error!("Failed to download media: {}", e);
            media_processor.ensure_download_recorded(&request, started_at, &Err(e.to_string()));
            return;
        }
    };

    let mut file = match File::create(path).await {
        Ok(file) => file,
        Err(e) => {
            tracing::error!("Failed to create file: {}", e);
            return;
        }
    };

    if let Err(e) = file.write_all(&content).await {
        tracing::error!("Failed to write file: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a few bytes to a uniquely named temp file for the clipboard
    /// paste tests, cleaning it up at the end.
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn with_bytes(name: &str, contents: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "deplace-paste-test-{}-{name}",
                uuid::Uuid::new_v4()
            ));
            std::fs::write(&path, contents).expect("temp file should be writable");
            Self(path)
        }

        fn file_url(&self) -> String {
            url::Url::from_file_path(&self.0)
                .expect("path should become a URL")
                .to_string()
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// A `file://` URI pasted as clipboard text must attach.
    #[tokio::test]
    async fn paste_of_a_file_uri_becomes_an_attachment() {
        let file = TempFile::with_bytes("screencast.webm", b"not really webm");
        let attachments = MatrixAttachment::from_clipboard_file_list(file.file_url()).await;
        assert_eq!(attachments.len(), 1, "the video should attach");
        assert_eq!(
            attachments[0].filename,
            file.0.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(attachments[0].mime_type, "video/webm");
    }

    /// Plain paths from `read_files` used to be dropped by `Url::parse`.
    #[tokio::test]
    async fn paste_of_a_plain_path_becomes_an_attachment() {
        let file = TempFile::with_bytes("screencast.webm", b"not really webm");
        let text = file.0.to_string_lossy().into_owned();
        let attachments = MatrixAttachment::from_clipboard_file_list(text).await;
        assert_eq!(attachments.len(), 1, "a bare path must attach");
        assert_eq!(
            attachments[0].filename,
            file.0.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(attachments[0].mime_type, "video/webm");
    }

    /// A mixed list, one URI and one plain path on separate lines, the
    /// shape a multiline clipboard paste produces, yields both files.
    #[tokio::test]
    async fn paste_of_mixed_paths_yields_all_attachments() {
        let a = TempFile::with_bytes("one.webm", b"a");
        let b = TempFile::with_bytes("two.png", b"b");
        let text = format!("{}\n{}", a.file_url(), b.0.to_string_lossy());
        let attachments = MatrixAttachment::from_clipboard_file_list(text).await;
        assert_eq!(attachments.len(), 2, "both files should attach");
        let names: Vec<&str> = attachments.iter().map(|a| a.filename.as_str()).collect();
        assert!(names.contains(&a.0.file_name().unwrap().to_str().unwrap()));
        assert!(names.contains(&b.0.file_name().unwrap().to_str().unwrap()));
    }

    /// A path that does not exist is skipped with a log line, never a
    /// panic or an empty-but-silent paste.
    #[tokio::test]
    async fn paste_of_a_missing_path_is_skipped() {
        let text = "/nonexistent/deplace-paste-test-missing.webm".to_string();
        let attachments = MatrixAttachment::from_clipboard_file_list(text).await;
        assert!(attachments.is_empty());
    }
}

/// Refuses a file that cannot be held in memory twice over, which is what
/// uploading it costs: the bytes plus the copy the SDK makes while sending.
fn check_upload_fits(path: &std::path::Path) -> Result<()> {
    let Ok(size) = std::fs::metadata(path).map(|m| m.len()) else {
        return Ok(());
    };
    let Some(available) = available_memory() else {
        return Ok(());
    };

    if size.saturating_mul(2) > available {
        anyhow::bail!(
            "{} needs about {} of free memory to upload, but only {} is available",
            format_bytes(size, DataSizeUnit::Bytes),
            format_bytes(size.saturating_mul(2), DataSizeUnit::Bytes),
            format_bytes(available, DataSizeUnit::Bytes),
        );
    }

    Ok(())
}

/// The memory the system can still hand out, on platforms that report it.
fn available_memory() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;

    meminfo.lines().find_map(|line| {
        let rest = line.strip_prefix("MemAvailable:")?;
        let kib: u64 = rest.split_whitespace().next()?.parse().ok()?;
        Some(kib * 1024)
    })
}
