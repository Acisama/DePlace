use iced::{Alignment, Length};
use macros::{iced_cache, iced_icon};

use crate::{
    common::*,
    components::{CopyTextExt, PushElement},
};

#[derive(Debug, Clone)]
pub enum ProfileMessage {
    CopyUserId(String),
    CreateDm(OwnedUserId),
}

impl CopyTextExt for ProfileMessage {
    fn copy_text(text: String) -> Self {
        Self::CopyUserId(text)
    }
}

pub enum ProfileAction {
    CopyUserId(String),
    CloseOverlayWithTask(Task<()>),
}

#[iced_cache(Clone, Debug)]
pub struct OverlayProfile {
    state: AppState,
    member: Member,

    bounds: Rectangle,
    is_own_user: bool,
}

impl OverlayProfile {
    pub fn new(state: AppState, member: Member, bounds: Rectangle) -> Self {
        Self {
            is_own_user: state.own_device_ref().user_id == member.user_id(),
            state,
            member,
            bounds,
        }
    }
}

impl IcedWidget<ProfileMessage, ProfileAction> for OverlayProfile {
    fn update(&mut self, message: ProfileMessage) -> Option<ProfileAction> {
        match message {
            ProfileMessage::CopyUserId(id) => Some(ProfileAction::CopyUserId(id)),
            ProfileMessage::CreateDm(id) => {
                let state = self.state.clone();
                Some(ProfileAction::CloseOverlayWithTask(Task::future(
                    async move {
                        state.change_or_create_dm(&id).await;
                    },
                )))
            }
        }
    }

    fn view(&self) -> iced::Element<'static, ProfileMessage> {
        let theme = theme();
        let structure = structure();

        let bounds = self.bounds;
        let member = self.member.clone();
        let is_own_user = self.is_own_user;

        let content_column = move || {
            let col = w::column![member.render_banner_column()].spacing(structure.small_gap);

            if !is_own_user {
                col.push_element(
                    w::container(
                        animated_button(
                            w::row![
                                w::text("Write Message").size(structure.font_size),
                                iced_icon!(envelope, BOLD, structure.font_size * 1.3)
                            ]
                            .align_y(Alignment::Center)
                            .spacing(structure.small_gap)
                            .padding(structure.small_gap),
                        )
                        .style(move |status| ButtonStyle {
                            background: if status.active() {
                                Some(theme.accent.into())
                            } else {
                                Some(theme.accent_blended().into())
                            },
                            text_color: if status.active() {
                                theme.solid_bg.into()
                            } else {
                                theme.accent.into()
                            },
                            border: Border {
                                color: if status.active() {
                                    theme.accent.into()
                                } else {
                                    theme.border.into()
                                },
                                width: structure.border_thickness,
                                radius: structure.inner_border_radius.into(),
                            },
                            ..Default::default()
                        })
                        .on_press(ProfileMessage::CreateDm(member.user_id().to_owned())),
                    )
                    .padding(padding::left(structure.gap)),
                )
            } else {
                col
            }
        };

        w::responsive(move |size| {
            let content: Element<'static, ProfileMessage> = floating_tile(
                theme,
                structure,
                w::container(content_column())
                    .style(move |_| ContainerStyle {
                        background: Some(theme.solid_bg.into()),
                        border: Border {
                            color: theme.border.into(),
                            width: structure.border_thickness,
                            radius: structure.outer_border_radius.into(),
                        },
                        ..Default::default()
                    })
                    .padding(padding::bottom(structure.gap)),
            )
            .into();

            let gap = structure.small_gap;
            let popup = w::container(content).padding(gap);

            // Estimate the popup's height from the structure constants that
            // drive its layout (banner column + "Message" row + padding),
            // since we don't have a measured size at this point.
            let sidebar = structure.chat.sidebar;
            let estimated_popup_height = sidebar.banner_height
                + sidebar.large_icon_size / 2.0
                + structure.large_font_size
                + structure.small_gap
                + structure.font_size
                + structure.small_gap
                + structure.font_size
                + structure.gap
                + structure.small_gap * 2.0;

            // Open toward the center horizontally. Vertically, anchor to the
            // top of the element, unless that would push the popup past the
            // bottom of the window, in which case anchor to the window's
            // bottom instead.
            let open_right = bounds.center_x() < size.width / 2.0;
            let open_down = bounds.y + estimated_popup_height <= size.height;

            let width = if open_right {
                Fill
            } else {
                Length::Fixed(bounds.x - gap)
            };

            w::container(popup)
                .width(width)
                .height(Fill)
                .align_x(if open_right {
                    Alignment::Start
                } else {
                    Alignment::End
                })
                .align_y(if open_down {
                    Alignment::Start
                } else {
                    Alignment::End
                })
                .padding(Padding {
                    top: if open_down { bounds.y } else { 0.0 },
                    left: if open_right {
                        bounds.x + bounds.width + gap
                    } else {
                        0.0
                    },
                    right: 0.0,
                    bottom: 0.0,
                })
        })
        .boxed()
    }
}
