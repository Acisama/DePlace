use std::io::Cursor;

use anyhow::Result;
use bytes::Bytes;
use matrix_sdk::media::MediaRequestParameters;
use rfd::{AsyncFileDialog, FileHandle};

use mime::Mime;
use ruma::events::room::MediaSource;
use tokio::{fs::File, io::AsyncWriteExt};

use crate::state::{AppState, media_fetcher::ProgressEntry};

#[derive(Clone, PartialEq, Eq)]
pub struct MatrixAttachment {
    pub filename: String,
    pub mime_type: Mime,
    pub data: Bytes,
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
        let read_start = std::time::Instant::now();
        let data = handle.read().await;
        tracing::info!(
            "Read {} ({} bytes) in {:?}",
            path.display(),
            data.len(),
            read_start.elapsed()
        );
        let mime_type = mime_guess::from_path(&path).first_or_octet_stream();
        Ok(Self::from_bytes(
            path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
            mime_type,
            data,
        ))
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

    pub async fn compressed_from_folder_picker(state: AppState) -> Option<MatrixAttachment> {
        let Some(folder_handle) = AsyncFileDialog::new()
            .set_title("Select folder to compress")
            .pick_folder()
            .await
        else {
            tracing::warn!("No folder selected");
            return None;
        };

        let folder_path = folder_handle.path().to_path_buf();
        let folder_name = folder_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder")
            .to_string();

        let media_processor = state.media_processor();
        let progress = media_processor.start_compression(folder_path.clone());
        let started_at = progress.started_at;

        let path_for_blocking = folder_path.clone();

        let data =
            match tokio::task::spawn_blocking(move || zip_folder(&path_for_blocking, &progress))
                .await
            {
                Ok(Ok(data)) => {
                    media_processor.finish_compression(folder_path, true, started_at);
                    data
                }
                Ok(Err(e)) => {
                    tracing::error!("Failed to compress folder: {}", e);
                    media_processor.finish_compression(folder_path, false, started_at);
                    return None;
                }
                Err(e) => {
                    tracing::error!("Compression task failed: {}", e);
                    media_processor.finish_compression(folder_path, false, started_at);
                    return None;
                }
            };

        tracing::info!(
            "Compressed {} ({} bytes) in {:?}",
            folder_name,
            data.len(),
            started_at.elapsed()
        );

        Some(Self::from_bytes(
            &format!("{folder_name}.zip"),
            "application/zip".parse().expect("valid mime type"),
            data,
        ))
    }
}

fn zip_folder(folder_path: &std::path::Path, progress: &ProgressEntry) -> Result<Vec<u8>> {
    use std::io::Write;

    let total: u64 = walkdir::WalkDir::new(folder_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();

    progress.set_total_bytes(total);
    progress.set_downloaded_bytes(0);

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

            progress.update_downloaded_bytes(|p| p + data.len() as u64);
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

    state.media_processor().add_relevant_source(&source);
    let request = MediaRequestParameters {
        source,
        format: matrix_sdk::media::MediaFormat::File,
    };

    let content = match state
        .client()
        .media()
        .get_media_content(&request, true)
        .await
    {
        Ok(content) => content,
        Err(e) => {
            tracing::error!("Failed to download media: {}", e);
            return;
        }
    };

    let mut file = match File::create(handle.path()).await {
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
