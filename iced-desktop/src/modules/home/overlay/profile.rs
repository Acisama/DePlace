use iced::Alignment;
use macros::iced_cache;

use crate::{
    common::*,
    components::{CopyUserIdExt, profile::render_banner_column},
};

#[derive(Debug, Clone)]
pub enum ProfileMessage {
    CopyUserId(OwnedUserId),
    UserIdCopied,
}

impl CopyUserIdExt for ProfileMessage {
    fn copy_user_id(id: OwnedUserId) -> Self {
        Self::CopyUserId(id)
    }
}

pub enum ProfileAction {
    CopyUserId(OwnedUserId),
}

#[iced_cache(Clone, Debug)]
pub struct OverlayProfile {
    member: Member,

    bounds: Rectangle,
}

impl OverlayProfile {
    pub fn new(_state: &AppState, member: Member, bounds: Rectangle) -> Self {
        Self { member, bounds }
    }
}

impl IcedWidget<ProfileMessage, ProfileAction> for OverlayProfile {
    fn update(&mut self, message: ProfileMessage) -> Option<ProfileAction> {
        match message {
            ProfileMessage::CopyUserId(id) => Some(ProfileAction::CopyUserId(id)),
            ProfileMessage::UserIdCopied => None,
        }
    }

    fn view(&self) -> iced::Element<'static, ProfileMessage> {
        let theme = theme();
        let structure = structure();

        let bounds = self.bounds;
        let member = self.member.clone();

        w::responsive(move |size| {
            let content: Element<'static, ProfileMessage> = floating_tile(
                theme,
                structure,
                w::container(render_banner_column(&member, theme, structure))
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

            if bounds.center_x() < size.width / 2.0 {
                // Bounds are left of center, so open toward the center (right).
                w::container(popup)
                    .width(Fill)
                    .height(Fill)
                    .padding(Padding {
                        top: bounds.y,
                        left: bounds.x + bounds.width + gap,
                        right: 0.0,
                        bottom: 0.0,
                    })
            } else {
                // Bounds are right of center, so open toward the center (left).
                w::container(popup)
                    .width(bounds.x - gap)
                    .height(Fill)
                    .align_x(Alignment::End)
                    .padding(Padding {
                        top: bounds.y,
                        ..Default::default()
                    })
            }
        })
        .boxed()
    }
}
