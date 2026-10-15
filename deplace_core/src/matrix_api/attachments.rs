use std::io::Cursor;

use anyhow::Result;
use bytes::Bytes;
use matrix_sdk::{Client, media::MediaRequestParameters};
use rfd::{AsyncFileDialog, FileHandle};

use mime::Mime;
use ruma::events::room::MediaSource;
use tokio::{fs::File, io::AsyncWriteExt};

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
        let dialog_start = std::time::Instant::now();
        let Some(file_handles) = AsyncFileDialog::new()
            .set_title("Select files")
            .pick_files()
            .await
        else {
            return Vec::new();
        };
        tracing::info!(
            "File picker dialog returned {} file(s) after {:?}",
            file_handles.len(),
            dialog_start.elapsed()
        );

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
}

pub async fn download_file_to_picked(source: MediaSource, file_name: String, client: Client) {
    let Some(handle) = AsyncFileDialog::new()
        .set_title("File destination")
        .set_file_name(file_name)
        .save_file()
        .await
    else {
        tracing::error!("No path to save file selected");
        return;
    };

    let request = MediaRequestParameters {
        source,
        format: matrix_sdk::media::MediaFormat::File,
    };

    let content = match client.media().get_media_content(&request, true).await {
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
