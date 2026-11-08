use std::{hash::Hash, sync::Arc};

use iced::widget::image::Handle as ImageHandle;
use matrix_sdk::{Client, media::MediaRequestParameters};
use ruma::{OwnedMxcUri, events::room::MediaSource};

use crate::{big_thumbnail_format, small_thumbnail_format};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Avatar {
    #[default]
    NoAvatar,
    Error(Arc<String>),
    Ok {
        image: ImageHandle,
        avatar_url: Arc<OwnedMxcUri>,
    },
}

impl Hash for Avatar {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::NoAvatar => 0.hash(state),
            Self::Error(e) => e.hash(state),
            Self::Ok { avatar_url, .. } => {
                avatar_url.hash(state);
            }
        }
    }
}

impl Avatar {
    pub(crate) async fn load_avatar(
        client: &Client,
        url: Option<OwnedMxcUri>,
        small: bool,
    ) -> Self {
        let Some(url) = url else {
            return Avatar::NoAvatar;
        };

        let request = MediaRequestParameters {
            source: MediaSource::Plain(url.clone()),
            format: if small {
                small_thumbnail_format()
            } else {
                big_thumbnail_format()
            },
        };

        // TODO: Somehow check if the avatar is svg
        match client.media().get_media_content(&request, true).await {
            Ok(bytes) => {
                // Decoding off a blocking worker keeps the draw thread from
                // having to do it while scrolling
                let image = match tokio::task::spawn_blocking(move || {
                    image::ImageReader::new(std::io::Cursor::new(&bytes))
                        .with_guessed_format()
                        .map_err(|e| tracing::error!("Failed to guess format of avatar: {e}"))
                        .ok()?
                        .decode()
                        .map_err(|e| tracing::error!("Failed to decode avatar: {e}"))
                        .ok()
                        .map(|decoded| {
                            let rgba = decoded.to_rgba8();
                            ImageHandle::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())
                        })
                })
                .await
                {
                    Ok(Some(image)) => Some(image),
                    Ok(None) => None,
                    Err(e) => {
                        tracing::error!("Failed to decode avatar: {e}");
                        None
                    }
                };

                match image {
                    Some(image) => Avatar::Ok {
                        image,
                        avatar_url: Arc::new(url),
                    },
                    None => Avatar::Error(Arc::from("Failed to decode avatar".to_string())),
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load avatar: {}", e);
                Avatar::Error(Arc::from(e.to_string()))
            }
        }
    }
}
