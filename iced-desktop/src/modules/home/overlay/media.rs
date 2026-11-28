use std::time::SystemTime;

use crate::{
    common::*,
    components::{render_media::render_image, render_media_label, video_controls},
    modules::home::chat::{ImageMessage, VideoMessage},
};
use chrono_tz::Tz;
use deplace_core::{
    formatting::format_message_long_date,
    media::cache::Image,
    settings::{DataSizeUnit, DateFormat, HourFormat},
};
use iced::{Alignment, Length};
use macros::iced_cache;
use matrix_sdk_ui::timeline::TimelineEventItemId;

#[derive(Debug, Clone)]
pub enum OverlayMediaType {
    Image {
        image: Arc<ImageMessage>,
        sender: Member,
        timestamp: SystemTime,
        identifier: TimelineEventItemId,
    },
    Avatar {
        image: Image,
        member: Member,
    },
    Video(Arc<VideoMessage>),
}

impl OverlayMediaType {
    pub fn image(
        image: Arc<ImageMessage>,
        sender: Member,
        timestamp: SystemTime,
        identifier: TimelineEventItemId,
    ) -> Self {
        OverlayMediaType::Image {
            image,
            sender,
            timestamp,
            identifier,
        }
    }

    pub fn avatar(image: Image, member: Member) -> Self {
        OverlayMediaType::Avatar { image, member }
    }

    pub fn video(video: Arc<VideoMessage>) -> Self {
        OverlayMediaType::Video(video)
    }
}

impl Hash for OverlayMediaType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            OverlayMediaType::Image { .. } => {}
            OverlayMediaType::Avatar { .. } => {}
            OverlayMediaType::Video { .. } => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum MediaOverlayMessage {
    Close,
    HelpHover(Option<HelpKey>),
    ExitFullscreenVideo,
}

impl HelpOverMessage for MediaOverlayMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        MediaOverlayMessage::HelpHover(help)
    }
}

pub enum MediaOverlayAction {
    Close,
    HelpHover(Option<HelpKey>),
    ExitFullscreenVideo,
}

#[iced_cache(Debug, Clone)]
pub struct MediaOverlay {
    #[hash]
    media: OverlayMediaType,

    timezone: Receiver<Tz>,
    hour_format: Receiver<HourFormat>,
    date_format: Receiver<DateFormat>,
    data_size_unit: Receiver<DataSizeUnit>,
}

impl PartialEq for MediaOverlay {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (&self.media, &other.media),
            (
                OverlayMediaType::Image { .. },
                OverlayMediaType::Image { .. }
            ) | (
                OverlayMediaType::Avatar { .. },
                OverlayMediaType::Avatar { .. }
            ) | (
                OverlayMediaType::Video { .. },
                OverlayMediaType::Video { .. }
            )
        )
    }
}

impl ExtraHash for MediaOverlay {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.timezone.borrow().hash(state);
        self.hour_format.borrow().hash(state);
        self.date_format.borrow().hash(state);
    }
}

impl MediaOverlay {
    pub fn new(state: &AppState, media: OverlayMediaType) -> Self {
        let settings = state.settings();

        Self {
            media,

            timezone: settings.timezone.watch(),
            hour_format: settings.hour_format.watch(),
            date_format: settings.date_format.watch(),
            data_size_unit: settings.data_size_unit.watch(),
        }
    }

    /// Whether the overlaid media is a video, so callers know to drive the
    /// window's fullscreen state alongside opening/closing this overlay.
    pub fn is_video(&self) -> bool {
        matches!(self.media, OverlayMediaType::Video(_))
    }
}

impl IcedWidget<MediaOverlayMessage, MediaOverlayAction> for MediaOverlay {
    fn update(&mut self, message: MediaOverlayMessage) -> Option<MediaOverlayAction> {
        match message {
            MediaOverlayMessage::ExitFullscreenVideo => {
                Some(MediaOverlayAction::ExitFullscreenVideo)
            }
            MediaOverlayMessage::HelpHover(help_key) => {
                Some(MediaOverlayAction::HelpHover(help_key))
            }
            MediaOverlayMessage::Close => Some(MediaOverlayAction::Close),
        }
    }

    fn view(&self) -> Element<'static, MediaOverlayMessage> {
        let theme = theme();
        let structure = structure();

        let header = structure.header;

        let (image, header_content, filename, size) = match &self.media {
            OverlayMediaType::Video(video) => {
                return w::container(
                    video_controls(video.video.clone(), video.fallback(theme))
                        .fullscreen(true)
                        .on_fullscreen_toggle(MediaOverlayMessage::ExitFullscreenVideo),
                )
                .width(Fill)
                .height(Fill)
                .boxed();
            }
            OverlayMediaType::Image {
                image,
                sender,
                timestamp,
                ..
            } => (
                image.image.clone(),
                w::row![
                    sender.render_icon(header.button_size),
                    w::column![
                        sender.render_name(structure.font_size),
                        w::text(format_message_long_date(
                            *timestamp,
                            *self.timezone.borrow(),
                            *self.hour_format.borrow(),
                            *self.date_format.borrow()
                        ))
                        .size(structure.small_font_size)
                        .color(theme.text.dim)
                    ]
                    .spacing(structure.small_gap / 4.0)
                ]
                .align_y(Alignment::Center)
                .spacing(structure.small_gap)
                .boxed(),
                image.image.filename().to_string(),
                image.image.size(),
            ),
            OverlayMediaType::Avatar { image, member } => (
                image.clone(),
                w::rich_text![w::span("Avatar of "), member.name_span()]
                    .size(structure.font_size)
                    .boxed(),
                format!("avatar_{}", member.user_id()),
                None,
            ),
        };

        w::column![
            floating_tile(
                theme,
                structure,
                w::row![
                    w::container(header_content).width(Length::FillPortion(1)),
                    w::container(render_media_label(
                        filename.clone(),
                        size,
                        *self.data_size_unit.borrow(),
                        structure.font_size
                    ))
                    .align_x(Alignment::Center)
                    .width(Length::FillPortion(1)),
                    w::row![
                        Space::new().width(Fill),
                        close_button(MediaOverlayMessage::Close)
                    ]
                    .width(Length::FillPortion(1))
                ]
                .align_y(Alignment::Center)
                .height(Fill)
                .width(Fill)
            )
            .width(Fill)
            .padding(padding::left(header.button_padding()))
            .height(header.height),
            w::container(render_image(image.clone(),).fallback(Box::new(
                move |_radius, opacity| {
                    w::container(
                        w::container(
                            w::text("Media loading...")
                                .color(theme.text.dim.to_iced().scale_alpha(opacity))
                                .size(structure.large_font_size),
                        )
                        .style(move |_| ContainerStyle {
                            background: Some(theme.solid_bg.to_iced().scale_alpha(opacity).into()),
                            border: Border {
                                color: theme.border.into(),
                                width: structure.border_thickness,
                                radius: structure.inner_border_radius.into(),
                            },
                            ..Default::default()
                        })
                        .padding(structure.small_gap),
                    )
                    .width(Fill)
                    .height(Fill)
                    .center(Fill)
                    .boxed()
                }
            )))
            .width(Fill)
            .height(Fill)
            .center(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .spacing(structure.gap)
        .boxed()
    }
}
