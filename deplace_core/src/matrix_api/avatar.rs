use std::{hash::Hash, sync::Arc};

use iced::widget::image::Handle as ImageHandle;
use matrix_sdk::{Client, media::MediaRequestParameters};
use ruma::{OwnedMxcUri, events::room::MediaSource};

use crate::thumbnail_format;

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
    pub(crate) async fn load_avatar(client: &Client, url: Option<OwnedMxcUri>) -> Self {
        let Some(url) = url else {
            return Avatar::NoAvatar;
        };

        let request = MediaRequestParameters {
            source: MediaSource::Plain(url.clone()),
            format: thumbnail_format(),
        };

        match client.media().get_media_content(&request, true).await {
            Ok(bytes) => Avatar::Ok {
                image: ImageHandle::from_bytes(bytes),
                avatar_url: Arc::new(url),
            },
            Err(e) => Avatar::Error(Arc::from(e.to_string())),
        }
    }
}
