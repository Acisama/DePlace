use std::hash::Hash;

use deplace_core::colors::ColorExt;
use deplace_core::state::cache::{AvatarCache, MediaState};
use deplace_core::{NameExt, RestoreResult};
use iced::font::Weight;
use iced::widget::canvas::{Frame, Path, Stroke};
use iced::widget::text::Rich;
use iced::widget::{self as w, Canvas, canvas, image, rich_text, span};
use iced::{
    Border, Element,
    Length::Fill,
    Subscription, Task,
    widget::{Container, Shader, Stack},
    window,
};
use iced::{Color, ContentFit, Font, Point, Renderer, Size};
use matrix_sdk::Room;
use matrix_sdk::room::RoomMember;
use matrix_sdk::ruma::OwnedMxcUri;

use crate::components::authentification::login::{LoginAction, LoginMessage};
use crate::components::authentification::verification::{
    Verification, VerificationAction, VerificationMessage,
};
use crate::components::home::HomeAction;
use crate::things::{Colors, Theme};
use crate::{AppMessage, things::Structure};
use authentification::{
    discovery::{Discovery, DiscoveryAction, DiscoveryMessage},
    login::Login,
};
use home::Home;

pub(crate) mod authentification;
pub(crate) mod home;
mod on_appear;
pub(crate) mod root;
pub(crate) mod shader;

pub use on_appear::on_appear;

pub enum GenericState<T: Clone> {
    Ready,
    Checking,
    Success(T),
    Error(String),
}

impl<T: Clone> Hash for GenericState<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            GenericState::Ready => 0.hash(state),
            GenericState::Checking => 1.hash(state),
            GenericState::Success(_) => 2.hash(state),
            GenericState::Error(e) => e.hash(state),
        }
    }
}

pub fn weighted_text<'a, T>(
    text: impl Into<String>,
    weight: iced::font::Weight,
) -> Rich<'a, (), T> {
    rich_text([span(text.into()).font(Font {
        weight,
        ..Default::default()
    })])
}

impl<T: Clone> GenericState<T> {
    pub fn success(&self) -> Option<T> {
        match self {
            GenericState::Success(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self, GenericState::Ready)
    }

    pub fn text<V>(
        &self,
        success: &str,
        ready: &str,
        colors: &Colors,
        structure: &Structure,
    ) -> Rich<'static, (), V> {
        let (text, color) = match self {
            GenericState::Ready => (ready.to_string(), colors.success),
            GenericState::Success(_) => (success.to_string(), colors.success),
            GenericState::Error(e) => (e.clone(), colors.error),
            GenericState::Checking => ("Checking...".to_string(), colors.offline),
        };

        weighted_text(text, iced::font::Weight::Semibold)
            .color(color)
            .size(structure.font_size)
    }
}

pub fn floating_tile<'a, T>(
    theme: Theme,
    structure: Structure,
    content: impl Into<Element<'a, T>>,
) -> Container<'a, T> {
    use w::container::Style;
    w::container(content).style(move |_theme| Style {
        background: Some(theme.background.into()),
        border: Border {
            color: theme.border,
            width: structure.border_thickness,
            radius: structure.outer_border_radius.into(),
        },
        ..Style::default()
    })
}

pub fn text_input<'a, T>(
    placeholder: &str,
    value: &str,
    theme: Theme,
    structure: Structure,
) -> w::text_input::TextInput<'a, T>
where
    T: Clone,
{
    use w::text_input::{Status, Style};

    w::text_input(placeholder, value)
        .size(structure.font_size)
        .padding(structure.small_gap)
        .style(move |_theme, status| Style {
            background: theme.background.into(),
            border: Border {
                color: if matches!(status, Status::Focused { .. }) {
                    theme.accent
                } else {
                    theme.border
                },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            placeholder: theme.text.muted,
            icon: theme.text.muted,
            selection: theme.text.muted,
            value: theme.text.normal,
        })
}

struct InsetShadow {
    radius: f32,
    color: Color,
    depth: f32,
    layers: usize,
}

impl InsetShadow {
    fn new(radius: f32, color: Color, depth: f32, layers: usize) -> Self {
        Self {
            radius,
            color,
            depth,
            layers,
        }
    }
}

impl<Message> canvas::Program<Message> for InsetShadow {
    type State = ();

    fn draw(
        &self,
        _: &Self::State,
        renderer: &Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::advanced::mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());

        for i in 0..self.layers {
            let t = i as f32 / self.layers as f32; // 0.0 at the edge, 1.0 at `depth`
            let inset = t * self.depth;
            let alpha = self.color.a * (1.0 - t).powf(2.0); // falls off quadratically inward

            let path = Path::rounded_rectangle(
                Point::new(inset, inset),
                Size::new(bounds.width - inset * 2.0, bounds.height - inset * 2.0),
                (self.radius - inset).max(0.0).into(), // shrink the radius as we inset, same as the outer shape
            );

            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(Color {
                        a: alpha,
                        ..self.color
                    })
                    .with_width(self.depth / self.layers as f32 * 1.5),
            );
        }

        vec![frame.into_geometry()]
    }
}

pub fn text_icon<'a, T: 'a>(
    text: impl Into<String> + iced::advanced::text::IntoFragment<'a>,
    size: f32,
    rounding: f32,
    color: Color,
) -> Element<'a, T> {
    Stack::new()
        .push(
            w::container(weighted_text(text, Weight::Bold).size(size / 2.0))
                .width(size)
                .height(size)
                .center(size)
                .style(move |_| w::container::Style {
                    background: Some(color.scale_alpha(0.2).into()),
                    text_color: Some(color),
                    border: Border {
                        color,
                        width: 0.0,
                        radius: rounding.into(),
                    },
                    ..Default::default()
                }),
        )
        .push(
            Canvas::new(InsetShadow::new(size / 4.0, color, size / 8.0, 8))
                .width(size)
                .height(size),
        )
        .into()
}

/// A trait for messages which have a NeedAvatar variant
pub trait NeedsAvatarExt {
    fn needs_avatar(uri: OwnedMxcUri) -> Self;
}

pub trait IconExt {
    fn render_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
    ) -> Element<'a, T>;
}

impl IconExt for Room {
    fn render_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
    ) -> Element<'a, T> {
        let rounding = size / 4.0;

        let fallback = move || text_icon(self.initial(), size, rounding, self.color().to_iced());

        let Some(avatar_url) = self.avatar_url() else {
            return fallback();
        };

        match avatar_cache.get(&avatar_url) {
            Some(MediaState::Failed) | Some(MediaState::Loading) => fallback(),
            Some(MediaState::Loaded(avatar)) => image((*avatar).clone())
                .width(size)
                .height(size)
                .content_fit(ContentFit::Cover)
                .border_radius(rounding)
                .into(),
            None => on_appear(fallback(), T::needs_avatar(avatar_url)).into(),
        }
    }
}

impl IconExt for RoomMember {
    fn render_icon<'a, T: NeedsAvatarExt + Clone + 'a>(
        &self,
        size: f32,
        avatar_cache: &AvatarCache,
    ) -> Element<'a, T> {
        let rounding = size / 2.0;

        let fallback = move || text_icon(self.initial(), size, rounding, self.color().to_iced());

        let Some(avatar_url) = self.avatar_url() else {
            return fallback();
        };

        match avatar_cache.get(&avatar_url.into()) {
            Some(MediaState::Failed) | Some(MediaState::Loading) => fallback(),
            Some(MediaState::Loaded(avatar)) => image((*avatar).clone())
                .width(size)
                .height(size)
                .content_fit(ContentFit::Cover)
                .border_radius(rounding)
                .into(),
            None => on_appear(fallback(), T::needs_avatar(avatar_url.to_owned())).into(),
        }
    }
}
