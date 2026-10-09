use std::time::SystemTime;

use crate::{
    common::*,
    components::{
        MediaType,
        home::chat::{ImageMessage, VideoMessage},
        render_media_failed_to_load,
    },
};
use chrono_tz::Tz;
use deplace_core::{
    formatting::format_message_long_date,
    settings::{DateFormat, HourFormat},
};
use iced::{Alignment, Length};
use macros::iced_cache;
use matrix_sdk::{media::UniqueKey, ruma::events::room::MediaSource};

#[derive(Debug, Clone, Hash)]
pub enum OverlayMediaType {
    Image(Arc<ImageMessage>),
    Video(Arc<VideoMessage>),
}

#[derive(Debug, Clone)]
pub enum MediaOverlayMessage {
    NeedsMedia(NeedsMedia),
    Close,
    HelpHover(Option<HelpKey>),
}

impl NeedsAvatarExt for MediaOverlayMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsMedia(NeedsMedia::avatar(uri))
    }
}

pub enum MediaOverlayAction {
    NeedsMedia(NeedsMedia),
    Close,
    HelpHover(Option<HelpKey>),
}

#[iced_cache(Debug, Clone)]
pub struct MediaOverlay {
    #[hash]
    media: OverlayMediaType,

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
        media: OverlayMediaType,
        sender: RoomMember,
        timestamp: SystemTime,
        event_id: OwnedEventId,
    ) -> Self {
        Self {
            media,

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
            MediaOverlayMessage::HelpHover(help_key) => {
                Some(MediaOverlayAction::HelpHover(help_key))
            }
            MediaOverlayMessage::Close => Some(MediaOverlayAction::Close),
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

        let OverlayMediaType::Image(image) = &self.media else {
            return w::column![].into();
        };

        let help_view = HelpView::new(help_state, theme, MediaOverlayMessage::HelpHover);

        w::column![
            floating_tile(
                theme,
                structure,
                w::row![
                    w::container(
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
                        .align_y(Alignment::Center)
                        .spacing(structure.small_gap)
                    )
                    .width(Length::FillPortion(1)),
                    w::container(
                        w::text(image.label())
                            .width(Fill)
                            .color(theme.text.normal)
                            .size(structure.font_size)
                            .center()
                    )
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
            w::container(render_image(theme, structure, image, &self.image_cache))
                .width(Fill)
                .height(Fill)
                .center(Fill)
        ]
        .width(Fill)
        .height(Fill)
        .spacing(structure.gap)
        .into()
    }
}

fn render_image(
    theme: Theme,
    structure: Structure,
    image: &Arc<ImageMessage>,
    image_cache: &ImageCache,
) -> iced::Element<'static, MediaOverlayMessage> {
    let result = image_cache.get(&image.source.unique_key());

    let fired = result.is_some();
    let media_state = result.unwrap_or(MediaState::Loading);

    let dimensions = media_state.as_option().and_then(|arc| arc.2);

    let width = dimensions
        .map(|(w, _)| w as f32)
        .or(image.info.as_ref().and_then(|i| i.width.map(|w| w as f32)))
        .unwrap_or(structure.chat.max_media_width);
    let height = dimensions
        .map(|(_, h)| h as f32)
        .or(image.info.as_ref().and_then(|i| i.height.map(|h| h as f32)))
        .unwrap_or(structure.chat.max_media_height);

    let mut stack_children = Vec::new();

    if let Some(image) = image.blur_preview.clone() {
        stack_children.push(
            w::image(image)
                .width(Fill)
                .height(Fill)
                .content_fit(iced::ContentFit::Contain)
                .border_radius(structure.inner_border_radius)
                .into(),
        );
    }

    match media_state {
        MediaState::Failed => {
            stack_children.push(render_media_failed_to_load(
                theme,
                structure,
                width,
                height,
                MediaType::Image,
            ));
        }
        MediaState::Loaded(img) => {
            stack_children.push(
                w::image(img.0.clone())
                    .width(Fill)
                    .height(Fill)
                    .content_fit(iced::ContentFit::Contain)
                    .border_radius(structure.inner_border_radius)
                    .into(),
            );
        }
        _ => {}
    }

    let stack = Stack::with_children(stack_children).into();

    if !fired {
        on_appear(
            stack,
            MediaOverlayMessage::NeedsMedia(NeedsMedia::Image {
                source: image.source.clone(),
            }),
        )
        .into()
    } else {
        stack
    }
}
