use std::time::SystemTime;

use crate::common::*;
use chrono_tz::Tz;
use deplace_core::{
    formatting::format_message_long_date,
    settings::{DateFormat, HourFormat},
};
use iced::Alignment;
use macros::iced_cache;
use matrix_sdk::{media::UniqueKey, ruma::events::room::MediaSource};

#[derive(Debug, Clone, Copy, Hash, PartialEq)]
pub enum MediaType {
    Image,
    Video,
}

impl DisplayString for MediaType {
    fn display_string(&self) -> String {
        match self {
            MediaType::Image => "Image".to_string(),
            MediaType::Video => "Video".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MediaOverlayMessage {
    NeedsMedia(NeedsMedia),
}

impl NeedsAvatarExt for MediaOverlayMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsMedia(NeedsMedia::avatar(uri))
    }
}

pub enum MediaOverlayAction {
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Debug, Clone)]
pub struct MediaOverlay {
    #[hash]
    media: MediaType,

    source: MediaSource,

    avatar_cache: AvatarCache,
    image_cache: ImageCache,
    video_cache: VideoCache,

    sender: RoomMember,
    timestamp: SystemTime,

    #[hash]
    event_id: OwnedEventId,

    timezone: Receiver<Tz>,
    hour_format: Receiver<HourFormat>,
    date_format: Receiver<DateFormat>,
}

impl ExtraHash for MediaOverlay {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.timezone.borrow().hash(state);
        self.hour_format.borrow().hash(state);
        self.date_format.borrow().hash(state);
    }
}

impl PartialEq for MediaOverlay {
    fn eq(&self, other: &Self) -> bool {
        self.event_id == other.event_id
    }
}

impl MediaOverlay {
    pub fn new(
        state: &AppState,
        media: MediaType,
        source: MediaSource,
        sender: RoomMember,
        timestamp: SystemTime,
        event_id: OwnedEventId,
    ) -> Self {
        Self {
            media,
            source,

            avatar_cache: state.avatar_cache().clone(),
            image_cache: state.image_cache().clone(),
            video_cache: state.video_cache().clone(),

            avatar_states_for_hash: Default::default(),
            image_states_for_hash: Default::default(),
            video_states_for_hash: Default::default(),

            sender,
            timestamp,
            event_id,

            timezone: state.settings().timezone.watch(),
            hour_format: state.settings().hour_format.watch(),
            date_format: state.settings().date_format.watch(),
        }
    }
}

impl IcedWidget<MediaOverlayMessage, MediaOverlayAction> for MediaOverlay {
    fn update(&mut self, message: MediaOverlayMessage) -> Option<MediaOverlayAction> {
        match message {
            MediaOverlayMessage::NeedsMedia(media) => {
                match &media {
                    NeedsMedia::Avatar { uri } => self.avatar_states_for_hash.insert(uri.clone()),
                    NeedsMedia::Image { source } => {
                        self.image_states_for_hash.insert(source.unique_key())
                    }
                    NeedsMedia::Video { source } => {
                        self.image_states_for_hash.insert(source.unique_key())
                    }
                    _ => return None,
                };
                Some(MediaOverlayAction::NeedsMedia(media))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, MediaOverlayMessage> {
        let header = structure.header;

        if self.media != MediaType::Image {
            return w::column![].into();
        }

        w::column![
            floating_tile(
                theme,
                structure,
                w::row![
                    self.sender
                        .render_icon(header.button_size, &self.avatar_cache),
                    w::column![
                        self.sender.render_name(structure.font_size),
                        w::text(format_message_long_date(
                            self.timestamp,
                            *self.timezone.borrow(),
                            *self.hour_format.borrow(),
                            *self.date_format.borrow()
                        ))
                        .size(structure.small_font_size)
                        .color(theme.text.dim)
                    ]
                    .spacing(structure.small_gap / 4.0)
                ]
                .spacing(structure.small_gap)
                .align_y(Alignment::Center)
                .height(Fill)
            )
            .width(Fill)
            .padding(padding::left(header.button_padding()))
            .height(header.height),
            w::container(w::text("Image")).width(Fill).height(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .spacing(structure.gap)
        .into()
    }
}
