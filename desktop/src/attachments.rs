use std::sync::Arc;

use crate::{
    components::{ByteSize, profiles::render_icon},
    things::AppTheme,
};
use chrono::{DateTime, Utc};
use gpui::{
    AnyElement, Element, Hsla, ObjectFit, ParentElement, Pixels, SharedString, Styled, StyledImage,
    img, px,
};
use gpui_component::Colorize;
use macros::tailwind_div;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use mime_guess::Mime;
use uuid::Uuid;

#[derive(Clone)]
pub enum AttachmentState {
    Loaded(Arc<Vec<u8>>),
    Failed(SharedString),
    Started,
}

#[derive(Clone)]
pub enum AttachmentPreview {
    Image(Arc<gpui::Image>),
    Extention {
        extension: SharedString,
        color: Hsla,
    },
    Unknown,
}

#[derive(Clone)]
pub struct Attachment {
    pub name: SharedString,
    pub size: ByteSize,
    pub mime_type: Arc<Mime>,
    pub state: AttachmentState,
    pub preview: AttachmentPreview,
}

#[derive(Clone)]
pub struct SearchParameters {
    pub search_id: Uuid,
    pub room_ids: Vec<OwnedRoomId>,
    pub text: String,
    pub senders: Vec<OwnedUserId>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub has_link: bool,
}

impl Attachment {
    pub fn new(name: &str, mime_type: Mime) -> Self {
        Self {
            name: name.into(),
            size: ByteSize::new(0),
            mime_type: Arc::new(mime_type),
            state: AttachmentState::Started,
            preview: AttachmentPreview::Unknown,
        }
    }

    pub fn render_preview(&self, theme: &AppTheme, rounding: Pixels) -> AnyElement {
        if let AttachmentState::Failed(_) = &self.state {
            return tailwind_div!(
                size_full,
                text_center,
                flex,
                items_center,
                justify_center,
                rounded_t(rounding),
                text_color(theme.colors.unknown),
                bg(theme.colors.unknown.lightness(0.1))
            )
            .child(render_icon(phosphor_svgs::icon::warning::BOLD, px(20.0)))
            .into_any();
        }

        match &self.preview {
            AttachmentPreview::Image(preview) => img(preview.clone())
                .size_full()
                .object_fit(ObjectFit::ScaleDown)
                .rounded_t(rounding)
                .into_any(),
            AttachmentPreview::Extention { color, extension } => tailwind_div!(
                size_full,
                text_center,
                flex,
                items_center,
                justify_center,
                rounded_t(rounding),
                text_color(*color),
                bg(color.lightness(0.1))
            )
            .child(extension.clone())
            .into_any(),
            AttachmentPreview::Unknown => tailwind_div!(
                size_full,
                text_center,
                flex,
                items_center,
                justify_center,
                rounded_t(rounding),
                text_color(theme.colors.unknown),
                bg(theme.colors.unknown.lightness(0.1))
            )
            .child("file")
            .into_any(),
        }
    }
}
