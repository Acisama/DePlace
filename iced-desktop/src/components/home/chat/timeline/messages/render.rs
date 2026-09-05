use deplace_core::formatting::{fit_dimensions, format_bytes};
use iced::{Alignment, Background, Length, gradient::Linear, never};
use iced_video_player::VideoPlayer;
use matrix_sdk::{
    media::UniqueKey,
    ruma::{
        events::{
            room::{guest_access::GuestAccess, history_visibility::HistoryVisibility},
            rtc::notification::CallIntent,
        },
        room::JoinRule,
    },
};
use matrix_sdk_ui::timeline::{MembershipChange, TimelineDetails};
use phosphor_svgs::icon as icons;

use crate::{
    common::*,
    components::{
        InsetShadow,
        home::chat::timeline::messages::{SystemEvent, SystemMessage},
    },
};

use super::{ImageMessage, MessageContent, MessageEvent, TimelineItemMessage, VideoMessage};

impl MessageEvent {
    pub fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, TimelineItemMessage> {
        let pre_col_width = structure.small_gap * 1.5;
        let text_size = structure.chat.text_size;

        let show_header = true;

        let (text_content, other_content) = self.content.view(theme, structure);

        let mut column = w::Column::new();

        if let Some(text_content) = text_content {
            column = column.push(text_content);
        }
        if let Some(other_content) = other_content {
            column = column.push(other_content);
        }

        let highlight_color = if self.is_highlighted {
            Some(theme.accent.scale_alpha(0.2))
        } else {
            None
        };

        let background = highlight_color.map(|c| {
            Background::Gradient(iced::Gradient::Linear(
                Linear::new(90.0)
                    .add_stop(0.0, c)
                    .add_stop(1.0, Color::TRANSPARENT),
            ))
        });

        let (icon, name) = if show_header {
            {
                let size = structure.chat.icon_size;
                let rounding = size / 2.0;

                match &self.sender_profile {
                    TimelineDetails::Error(_) | TimelineDetails::Unavailable => (
                        Some(unknown_icon(size, rounding, theme)),
                        Some(render_unknown_name(size, theme)),
                    ),
                    TimelineDetails::Pending => (
                        Some(loading_icon(size, rounding, theme)),
                        Some(render_loading_name(size, theme)),
                    ),
                    TimelineDetails::Ready(p) => (
                        Some(p.render_icon(size, &self.avatar_cache)),
                        Some(p.render_name(text_size)),
                    ),
                }
            }
        } else {
            (None, None)
        };

        w::container(w::row![
            w::row![
                if self.is_highlighted {
                    w::container(
                        w::container("")
                            .width(pre_col_width / 3.0)
                            .height(Length::Fill)
                            .style(move |_| ContainerStyle {
                                border: border::rounded(pre_col_width / 6.0),
                                background: Some(theme.accent.into()),
                                ..Default::default()
                            }),
                    )
                    .padding(pre_col_width / 3.0)
                } else {
                    w::container("").width(pre_col_width)
                },
                w::row![
                    icon.unwrap_or(Space::new().width(structure.chat.icon_size).into()),
                    Space::new().width(pre_col_width),
                    w::column![name.unwrap_or(Space::new().into()), column]
                ]
                .padding(padding::vertical(structure.small_gap)),
            ]
            .height(Length::Shrink)
        ])
        .style(move |_| ContainerStyle {
            background,
            border: border::rounded(structure.semi_border_radius() - structure.border_thickness),
            ..Default::default()
        })
        .width(Fill)
        .into()
    }
}

impl MessageContent {
    fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> (
        Option<Element<'static, TimelineItemMessage>>,
        Option<Element<'static, TimelineItemMessage>>,
    ) {
        let text_size = structure.chat.text_size;

        let render_text_color =
            |text: String, color: Color| w::text(text).color(color).size(text_size);
        let render_normal_text = |text: String| render_text_color(text, theme.text.normal);
        let render_warning_text = |text: &'static str| {
            (
                Some(render_text_color(text.to_string(), theme.colors.warning).into()),
                None,
            )
        };
        let render_error_text = |text: &'static str| {
            (
                Some(render_text_color(text.to_string(), theme.colors.error).into()),
                None,
            )
        };
        let itallic_text = |text: String| {
            (
                Some(
                    w::rich_text![w::span(text).font(Font {
                        style: iced::font::Style::Italic,
                        ..Default::default()
                    })]
                    .color(theme.text.dim)
                    .on_link_click(never)
                    .size(text_size)
                    .into(),
                ),
                None,
            )
        };

        match &self {
            MessageContent::Audio => render_warning_text("Audio messages are not yet implemented"),
            MessageContent::Emote {
                body,
                formatted_body,
            } => (
                body.as_ref().map(|t| render_normal_text(t.clone()).into()),
                None,
            ),
            MessageContent::Empty => itallic_text("Empty".to_string()),
            MessageContent::File { .. } => {
                render_warning_text("File messages are not yet implemented")
            }
            MessageContent::Image { image, is_hovered } => {
                let mut stack = Stack::new().push(w::lazy(image.clone(), move |image| {
                    image.image_view(theme, structure)
                }));

                if *is_hovered {
                    stack = stack.push(
                        w::container(
                            w::container(
                                w::text(image.label())
                                    .size(structure.chat.text_size)
                                    .color(theme.text.normal),
                            )
                            .style(move |_| ContainerStyle {
                                background: Some(theme.solid_bg.into()),
                                border: Border {
                                    color: theme.border,
                                    width: structure.border_thickness,
                                    radius: ((structure.smaller_border_radius
                                        + structure.inner_border_radius)
                                        / 2.0)
                                        .into(),
                                },
                                ..Default::default()
                            })
                            .padding(structure.small_gap / 2.0),
                        )
                        .height(image.get_dimensions(structure).1)
                        .padding(structure.small_gap / 2.0)
                        .align_y(Alignment::End),
                    )
                }

                (
                    // TODO: Actually use image caption
                    None,
                    Some(
                        w::mouse_area(stack)
                            .on_enter(TimelineItemMessage::MediaMouseEnter)
                            .on_exit(TimelineItemMessage::MediaMouseLeave)
                            .interaction(Interaction::Pointer)
                            .into(),
                    ),
                )
            }
            MessageContent::LiveLocation => {
                render_warning_text("Live location messages are not yet implemented")
            }
            MessageContent::Location => {
                render_warning_text("Location messages are not yet implemented")
            }
            MessageContent::Notice { .. } => {
                render_warning_text("Notice messages are not yet implemented")
            }
            MessageContent::Other { event_type } => {
                itallic_text(format!("Message of type: {}", event_type))
            }
            MessageContent::ServerNotice { .. } => {
                render_warning_text("Server notice messages are not yet implemented")
            }
            MessageContent::Poll => render_warning_text("Poll messages are not yet implemented"),
            MessageContent::Redacted => (
                Some(
                    w::row![
                        w::container(phosphor_icon(icons::trash::BOLD, text_size))
                            .style(move |_| ContainerStyle::default().color(theme.text.dim)),
                        w::rich_text![w::span("Redacted").font(Font {
                            style: iced::font::Style::Italic,
                            ..Default::default()
                        })]
                        .color(theme.text.dim)
                        .on_link_click(never)
                        .size(text_size)
                    ]
                    .spacing(structure.small_gap / 2.0)
                    .align_y(Alignment::Center)
                    .into(),
                ),
                None,
            ),
            MessageContent::Sticker => {
                render_warning_text("Sticker messages are not yet implemented")
            }
            MessageContent::Text {
                body,
                formatted_body,
                ..
            } => (
                body.as_ref().map(|t| render_normal_text(t.clone()).into()),
                None,
            ),
            MessageContent::UnableToDecrypt => {
                render_error_text("Unable to decrypt messages are not yet implemented")
            }
            MessageContent::VerificationRequest {
                body,
                formatted_body,
            } => (
                body.as_ref().map(|t| render_normal_text(t.clone()).into()),
                None,
            ),
            MessageContent::Video { video, is_hovered } => {
                let mut stack = Stack::new().push(w::lazy(video.clone(), move |video| {
                    video.video_view(theme, structure)
                }));

                if *is_hovered {
                    stack = stack.push(
                        w::container(
                            w::container(
                                w::text(video.label())
                                    .size(structure.chat.text_size)
                                    .color(theme.text.normal),
                            )
                            .style(move |_| ContainerStyle {
                                background: Some(theme.solid_bg.into()),
                                border: Border {
                                    color: theme.border,
                                    width: structure.border_thickness,
                                    radius: ((structure.smaller_border_radius
                                        + structure.inner_border_radius)
                                        / 2.0)
                                        .into(),
                                },
                                ..Default::default()
                            })
                            .padding(structure.small_gap / 2.0),
                        )
                        .height(video.get_dimensions(structure).1)
                        .padding(structure.small_gap / 2.0)
                        .align_y(Alignment::End),
                    )
                }

                (
                    // TODO: Actually use image caption
                    None,
                    Some(
                        w::mouse_area(stack)
                            .on_enter(TimelineItemMessage::MediaMouseEnter)
                            .on_exit(TimelineItemMessage::MediaMouseLeave)
                            .interaction(Interaction::Pointer)
                            .into(),
                    ),
                )
            }
        }
    }
}

impl ImageMessage {
    fn label(&self) -> String {
        format!(
            "{}{}",
            self.filename,
            self.info
                .as_ref()
                .and_then(|i| i.size.map(|s| format!(
                    " ({})",
                    format_bytes(s, deplace_core::settings::DataSizeUnit::Bytes)
                )))
                .unwrap_or_default()
        )
    }

    fn get_dimensions(&self, structure: Structure) -> (f32, f32) {
        let max_width = structure.chat.max_media_width;
        let max_height = structure.chat.max_media_height;
        let info = &self.info;

        let min_width = self.label().len() as f32 * structure.chat.text_size;
        let min_height = structure.chat.text_size * 2.0;

        fit_dimensions(
            info.as_ref()
                .and_then(|i| i.width.map(|w| w as f32))
                .unwrap_or(max_width),
            info.as_ref()
                .and_then(|i| i.height.map(|h| h as f32))
                .unwrap_or(max_height),
            max_width,
            max_height,
            min_width,
            min_height,
        )
    }

    fn image_view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> Element<'static, TimelineItemMessage> {
        let info = &self.info;

        let (width, height) = self.get_dimensions(structure);

        let thumbnail_source = info
            .as_ref()
            .and_then(|i| i.thumbnail_source.clone())
            .unwrap_or_else(|| self.source.clone());

        let thumbnail_key = (thumbnail_source.unique_key(), width as u64, height as u64);
        let image = self.thumbnail_cache.get(&thumbnail_key);

        let mut stack = Stack::new();

        if let Some(image) = self.blur_preview.clone() {
            stack = stack.push(
                w::image(image)
                    .width(width)
                    .height(height)
                    .content_fit(iced::ContentFit::Fill)
                    .border_radius(structure.inner_border_radius),
            );
        }

        stack = match image.as_ref().unwrap_or(&MediaState::Loading) {
            MediaState::Failed => stack
                .push(
                    w::container(
                        weighted_text("Image failed to load", Weight::Bold)
                            .size(structure.chat.text_size * 1.5),
                    )
                    .width(width)
                    .height(height)
                    .center(height)
                    .style(move |_| w::container::Style {
                        background: Some(theme.colors.error.scale_lightness(0.2).into()),
                        text_color: Some(theme.colors.error),
                        border: Border {
                            color: theme.colors.error,
                            width: 0.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
                )
                .push(
                    Canvas::new(InsetShadow::new(
                        structure.inner_border_radius,
                        theme.colors.error,
                        structure.chat.text_size / 2.0,
                        8,
                    ))
                    .width(width)
                    .height(height),
                ),
            MediaState::Loaded(image) => stack.push(
                w::image((*(*image).clone()).clone())
                    .width(width)
                    .height(height)
                    .border_radius(structure.inner_border_radius),
            ),
            _ => stack,
        };

        if image.is_none() {
            stack = stack.push(on_appear(
                Space::new(),
                TimelineItemMessage::NeedsMedia(NeedsMedia::thumbnail(
                    self.source.clone(),
                    thumbnail_key,
                )),
            ))
        }

        let media = w::mouse_area(w::container(stack).width(width).height(height).style(
            move |_| ContainerStyle {
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: structure.inner_border_radius.into(),
                },
                ..Default::default()
            },
        ));

        media.into()
    }
}

impl VideoMessage {
    fn label(&self) -> String {
        format!(
            "{}{}",
            self.filename,
            self.info
                .as_ref()
                .and_then(|i| i.size.map(|s| format!(
                    " ({})",
                    format_bytes(s, deplace_core::settings::DataSizeUnit::Bytes)
                )))
                .unwrap_or_default()
        )
    }

    fn get_dimensions(&self, structure: Structure) -> (f32, f32) {
        let max_width = structure.chat.max_media_width;
        let max_height = structure.chat.max_media_height;
        let info = &self.info;

        let min_width = self.label().len() as f32 * structure.chat.text_size;
        let min_height = structure.chat.text_size * 2.0;

        // Matrix doesn't require senders to include dimensions in `info`,
        // so when they're missing, assume a typical 16:9 video instead of
        // the full (usually squarer) bounding box -- otherwise `scale`
        // below resolves to exactly 1.0 and the placeholder stretches to
        // fill `max_height`, which looks far too tall for real video.
        const DEFAULT_ASPECT_RATIO: f32 = 16.0 / 9.0;
        let (natural_width, natural_height) = match (
            info.as_ref().and_then(|i| i.width),
            info.as_ref().and_then(|i| i.height),
        ) {
            (Some(w), Some(h)) => (w as f32, h as f32),
            _ => (max_width, max_width / DEFAULT_ASPECT_RATIO),
        };

        fit_dimensions(
            natural_width,
            natural_height,
            max_width,
            max_height,
            min_width,
            min_height,
        )
    }

    fn video_view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> Element<'static, TimelineItemMessage> {
        let (width, height) = self.get_dimensions(structure);

        let cached_video = self.video_cache.get(&self.source.unique_key());
        let mut stack = Stack::new();

        if let Some(image) = self.blur_preview.clone() {
            stack = stack.push(
                w::image(image)
                    .width(width)
                    .height(height)
                    .content_fit(iced::ContentFit::Fill)
                    .border_radius(structure.inner_border_radius),
            );
        }

        stack = match cached_video.as_ref().unwrap_or(&MediaState::Loading) {
            MediaState::Loaded(video) => stack.push(
                w::mouse_area(
                    VideoPlayer::<TimelineItemMessage>::new(video.0.clone())
                        .width(width)
                        .height(height),
                )
                .on_press(TimelineItemMessage::ToggleVideoPause(video.0.clone())),
            ),
            MediaState::Failed => stack
                .push(
                    w::container(
                        weighted_text("Video failed to load", Weight::Bold)
                            .size(structure.chat.text_size * 1.5),
                    )
                    .width(width)
                    .height(height)
                    .center(height)
                    .style(move |_| w::container::Style {
                        background: Some(theme.colors.error.scale_lightness(0.2).into()),
                        text_color: Some(theme.colors.error),
                        border: Border {
                            color: theme.colors.error,
                            width: 0.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
                )
                .push(
                    Canvas::new(InsetShadow::new(
                        structure.inner_border_radius,
                        theme.colors.error,
                        structure.chat.text_size / 2.0,
                        8,
                    ))
                    .width(width)
                    .height(height),
                ),
            MediaState::Loading => stack.push(
                w::container(
                    weighted_text("Loading Video...", Weight::Normal)
                        .size(structure.chat.text_size),
                )
                .width(width)
                .height(height)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
            ),
        };

        if cached_video.is_none() {
            stack = stack.push(on_appear(
                Space::new(),
                TimelineItemMessage::NeedsMedia(NeedsMedia::Video {
                    source: self.source.clone(),
                }),
            ))
        }

        stack.into()
    }
}

impl SystemEvent {
    pub fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> Element<'static, TimelineItemMessage> {
        let (icon, icon_color) = self.content.icon(theme);

        let col_width = structure.chat_col_width();
        let text_size = structure.chat.text_size;

        let sender_name = move || match &self.sender_profile {
            TimelineDetails::Error(_) | TimelineDetails::Unavailable => {
                render_unknown_name(text_size, theme)
            }
            TimelineDetails::Pending => render_loading_name(text_size, theme),
            TimelineDetails::Ready(p) => p.render_name(text_size),
        };

        w::row![
            w::container(phosphor_icon(icon, structure.chat.text_size))
                .style(move |_| ContainerStyle::default().color(icon_color))
                .width(col_width)
                .center_x(col_width),
            self.content.render_content(theme, structure, sender_name)
        ]
        .align_y(Alignment::Center)
        .into()
    }
}

impl SystemMessage {
    fn icon(&self, theme: Theme) -> (&'static str, Color) {
        match self {
            SystemMessage::CallInvite => (icons::phone_call::FILL, theme.text.dim),
            SystemMessage::RtcNotification { call_intent, .. } => (
                match call_intent {
                    Some(CallIntent::Video) => icons::video_camera::FILL,
                    _ => icons::phone_call::FILL,
                },
                theme.text.dim,
            ),
            // TODO: Add specific join/leave icons
            SystemMessage::MembershipChange(_) => (icons::users::FILL, theme.text.dim),
            SystemMessage::ProfileChange(_) => (icons::user_circle::FILL, theme.text.dim),
            SystemMessage::Custom { .. } => (icons::code::FILL, theme.colors.warning),
            SystemMessage::PolicyRuleRoom => (icons::scales::FILL, theme.text.dim),
            SystemMessage::PolicyRuleServer => (icons::database::FILL, theme.text.dim),
            SystemMessage::PolicyRuleUser => (icons::shield_check::FILL, theme.text.dim),
            SystemMessage::RoomAvatar(_) => (icons::image::FILL, theme.text.dim),
            SystemMessage::RoomCanonicalAlias(_) => (icons::at::FILL, theme.text.dim),
            SystemMessage::RoomCreate => (icons::plus_circle::FILL, theme.text.dim),
            SystemMessage::RoomEncryption => (icons::lock_key::FILL, theme.text.dim),
            SystemMessage::RoomGuestAccess(change) => (
                match change.current() {
                    Some(GuestAccess::CanJoin) => icons::door_open::FILL,
                    _ => icons::door::FILL,
                },
                theme.text.dim,
            ),
            SystemMessage::RoomHistoryVisibility(change) => (
                match change.current() {
                    Some(HistoryVisibility::Invited) => icons::eye::FILL,
                    Some(HistoryVisibility::Shared) => icons::eye_slash::FILL,
                    _ => icons::eye_closed::FILL,
                },
                theme.text.dim,
            ),
            SystemMessage::RoomJoinRules(change) => (
                match change.current() {
                    Some(JoinRule::Public) => icons::shield::FILL,
                    Some(JoinRule::Private) => icons::shield_slash::FILL,
                    Some(JoinRule::Knock) => icons::shield_checkered::FILL,
                    Some(JoinRule::Restricted(_)) | Some(JoinRule::KnockRestricted(_)) => {
                        icons::shield_warning::FILL
                    }
                    _ => icons::shield_slash::FILL,
                },
                theme.text.dim,
            ),
            SystemMessage::RoomName(_) => (icons::tag::FILL, theme.text.dim),
            SystemMessage::RoomPinnedEvents => (icons::push_pin::FILL, theme.text.dim),
            SystemMessage::RoomPowerLevels => (icons::lightning::FILL, theme.text.dim),
            SystemMessage::RoomServerAcl => (icons::prohibit::FILL, theme.text.dim),
            SystemMessage::RoomThirdPartyInvite => (icons::ticket::FILL, theme.text.dim),
            SystemMessage::RoomTombstone => (icons::skull::FILL, theme.text.dim),
            SystemMessage::RoomTopic(_) => (icons::chats::FILL, theme.text.dim),
            SystemMessage::SpaceChild => (icons::git_commit::FILL, theme.text.dim),
            SystemMessage::SpaceParent => (icons::tree_structure::FILL, theme.text.dim),
        }
    }

    fn render_content(
        &self,
        theme: Theme,
        structure: Structure,
        sender_name: impl Fn() -> Element<'static, TimelineItemMessage>,
    ) -> Element<'static, TimelineItemMessage> {
        let text_size = structure.chat.text_size;

        let basic_text = move |text: String| {
            w::row![
                sender_name(),
                w::text(" ").size(text_size),
                w::text(text).size(text_size).color(theme.text.dim)
            ]
            .into()
        };

        match self {
            SystemMessage::CallInvite => basic_text("invited to a call".to_string()),
            SystemMessage::Custom { event_type } => {
                basic_text(format!("sent an event of type {}", event_type))
            }
            SystemMessage::MembershipChange(change) => basic_text(
                match change.change() {
                    Some(MembershipChange::None) => "had no membership change",
                    Some(MembershipChange::Banned) => "was banned",
                    Some(MembershipChange::Joined) => "joined the room",
                    Some(MembershipChange::Invited) => "was invited",
                    Some(MembershipChange::Left) => "left the room",
                    Some(MembershipChange::Kicked) => "was kicked",
                    Some(MembershipChange::Error) => "had a membership change",
                    Some(MembershipChange::InvitationAccepted) => "accepted the invitation",
                    Some(MembershipChange::InvitationRejected) => "rejected the invitation",
                    Some(MembershipChange::InvitationRevoked) => "had their invitation revoked",
                    Some(MembershipChange::KickedAndBanned) => "was kicked and banned",
                    Some(MembershipChange::KnockAccepted) => "accepted the knock",
                    Some(MembershipChange::KnockDenied) => "denied the knock",
                    Some(MembershipChange::KnockRetracted) => "retracted the knock",
                    Some(MembershipChange::Knocked) => "knocked on the door",
                    Some(MembershipChange::NotImplemented) => "had a membership change",
                    Some(MembershipChange::Unbanned) => "was unbanned",
                    None => "changed membership",
                }
                .to_string(),
            ),
            SystemMessage::ProfileChange(change) => basic_text(change.display_string()),
            SystemMessage::PolicyRuleRoom => basic_text("changed the room's policy".to_string()),
            SystemMessage::PolicyRuleServer => {
                basic_text("changed the server's policy".to_string())
            }
            SystemMessage::PolicyRuleUser => basic_text("changed their policy".to_string()),
            SystemMessage::RoomAvatar(change) => {
                basic_text(change.display_string("the room's avatar"))
            }
            SystemMessage::RoomCanonicalAlias(change) => {
                basic_text(change.display_string("the room's canonical alias"))
            }
            SystemMessage::RoomCreate => basic_text("created the room".to_string()),
            SystemMessage::RoomEncryption => basic_text("enabled room encryption".to_string()),
            SystemMessage::RoomGuestAccess(change) => {
                basic_text(change.display_string("the room's guest access"))
            }
            SystemMessage::RoomHistoryVisibility(change) => {
                basic_text(change.display_string("the room's history visibility"))
            }
            SystemMessage::RoomJoinRules(change) => {
                basic_text(change.display_string_with_render_fn(
                    |c| c.as_str().to_string(),
                    "the room's join rules",
                ))
            }
            SystemMessage::RoomName(change) => basic_text(change.display_string("the room's name")),
            SystemMessage::RoomPinnedEvents => {
                basic_text("changed the room's pinned events".to_string())
            }
            SystemMessage::RoomPowerLevels => {
                basic_text("changed the room's power levels".to_string())
            }
            SystemMessage::RoomServerAcl => basic_text("changed the room's server ACL".to_string()),
            SystemMessage::RoomThirdPartyInvite => {
                basic_text("changed the room's third party invite".to_string())
            }
            SystemMessage::RoomTombstone => basic_text("changed the room's tombstone".to_string()),
            SystemMessage::RoomTopic(change) => {
                basic_text(change.display_string("the room's topic"))
            }
            SystemMessage::SpaceChild => basic_text("changed the space's child".to_string()),
            SystemMessage::SpaceParent => basic_text("changed the space's parent".to_string()),
            SystemMessage::RtcNotification { call_intent, .. } => basic_text(
                match call_intent {
                    Some(CallIntent::Video) => "started a video call",
                    _ => "started an audio call",
                }
                .to_string(),
            ),
        }
    }
}
