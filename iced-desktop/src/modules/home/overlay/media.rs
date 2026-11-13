use std::time::SystemTime;

use crate::{
    common::*,
    components::{render_media::render_image, render_media_label, video_controls},
    modules::home::chat::{ImageMessage, VideoMessage},
};
use chrono_tz::Tz;
use deplace_core::{
    formatting::format_message_long_date,
    settings::{DateFormat, HourFormat},
};
use iced::{Alignment, Length};
use macros::iced_cache;
use matrix_sdk_ui::timeline::TimelineEventItemId;

#[derive(Debug, Clone, Hash)]
pub enum OverlayMediaType {
    Image(Arc<ImageMessage>),
    Video(Arc<VideoMessage>),
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

    sender: Member,
    timestamp: SystemTime,

    identifier: TimelineEventItemId,

    timezone: Receiver<Tz>,
    hour_format: Receiver<HourFormat>,
    date_format: Receiver<DateFormat>,
}

impl PartialEq for MediaOverlay {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier
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
    pub fn new(
        state: &AppState,
        media: OverlayMediaType,
        sender: Member,
        timestamp: SystemTime,
        identifier: TimelineEventItemId,
    ) -> Self {
        Self {
            media,

            sender,
            timestamp,
            identifier,

            timezone: state.settings().timezone.watch(),
            hour_format: state.settings().hour_format.watch(),
            date_format: state.settings().date_format.watch(),
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

        let image = match &self.media {
            OverlayMediaType::Image(image) => image,
            OverlayMediaType::Video(video) => {
                return w::container(
                    video_controls(
                        video.video.clone(),
                        video.fallback(theme),
                        video.media_processor.clone(),
                        video.parameters_key.clone(),
                        video.info.as_ref().and_then(|i| i.size),
                    )
                    .fullscreen(true)
                    .on_fullscreen_toggle(MediaOverlayMessage::ExitFullscreenVideo),
                )
                .width(Fill)
                .height(Fill)
                .boxed();
            }
        };

        w::column![
            floating_tile(
                theme,
                structure,
                w::row![
                    w::container(
                        w::row![
                            self.sender.render_icon(header.button_size),
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
                        .align_y(Alignment::Center)
                        .spacing(structure.small_gap)
                    )
                    .width(Length::FillPortion(1)),
                    w::container(render_media_label(
                        image.filename.clone(),
                        image.info.as_ref().and_then(|i| i.size),
                        *image.data_size_unit.borrow(),
                        theme,
                        structure.font_size
                    ))
                    .width(Length::FillPortion(1)),
                    w::row![
                        Space::new().width(Fill),
                        close_button(theme, structure, MediaOverlayMessage::Close)
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
            w::container(render_image(image.image.clone()).fallback(Box::new(
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
