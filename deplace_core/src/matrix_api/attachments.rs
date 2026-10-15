use std::io::Cursor;

use anyhow::Result;
use rfd::{AsyncFileDialog, FileHandle};

use mime::Mime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixAttachment {
    pub filename: String,
    pub mime_type: Mime,
    pub data: Vec<u8>,
}

impl MatrixAttachment {
    pub fn from_bytes(filename: &str, mime_type: Mime, data: Vec<u8>) -> Self {
        Self {
            filename: filename.to_string(),
            mime_type,
            data,
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
        let data = handle.read().await;
        let path = handle.path();
        let mime_type = mime_guess::from_path(path).first_or_octet_stream();
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
}
