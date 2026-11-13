use std::time::SystemTime;

use crate::{
    common::*,
    components::{render_media::render_image, render_media_label},
    modules::home::chat::ImageMessage,
};
use chrono_tz::Tz;
use deplace_core::{
    cache::video::DePlaceVideoShared,
    formatting::format_message_long_date,
    settings::{DateFormat, HourFormat},
};
use iced::{Alignment, Length};
use macros::iced_cache;

#[derive(Debug, Clone, Hash)]
pub enum OverlayMediaType {
    Image(Arc<ImageMessage>),
    Video(DePlaceVideoShared),
}

#[derive(Debug, Clone)]
pub enum MediaOverlayMessage {
    Close,
    HelpHover(Option<HelpKey>),
}

impl HelpOverMessage for MediaOverlayMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        MediaOverlayMessage::HelpHover(help)
    }
}

pub enum MediaOverlayAction {
    Close,
    HelpHover(Option<HelpKey>),
}

#[iced_cache(Debug, Clone)]
pub struct MediaOverlay {
    #[hash]
    media: OverlayMediaType,

    sender: Member,
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
        media: OverlayMediaType,
        sender: Member,
        timestamp: SystemTime,
        event_id: OwnedEventId,
    ) -> Self {
        Self {
            media,

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
            MediaOverlayMessage::HelpHover(help_key) => {
                Some(MediaOverlayAction::HelpHover(help_key))
            }
            MediaOverlayMessage::Close => Some(MediaOverlayAction::Close),
        }
    }

    fn view(&self) -> iced::Element<'static, MediaOverlayMessage> {
        let theme = theme();
        let structure = structure();

        let header = structure.header;

        let OverlayMediaType::Image(image) = &self.media else {
            return w::space().boxed();
        };

        // let help_view = HelpView::new(theme);

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
