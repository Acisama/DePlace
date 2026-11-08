use std::hash::Hash;

use deplace_core::{
    helpers::DisplayString,
    structure::Structure,
    theme::{Colors, Theme},
};
use iced::{
    Background, Border, Color, Element, Fill, Font, Point, Renderer, Size, Widget as _,
    advanced::{Widget, layout, svg::Renderer as SvgRenderer, widget::Meta},
    border::{self, Radius},
    font::{Family, Weight},
    gradient::ColorStop,
    widget::{
        self as w, Canvas, Column, Container, Row, Scrollable, Stack,
        canvas::{self, Frame, Path, Stroke},
        image::Handle as ImageHandle,
        rich_text, span, svg,
        text::{IntoFragment, Rich},
    },
};
use matrix_sdk::ruma::{OwnedUserId, serde::Base64};

pub mod animated_button;
pub mod animation_clock;
pub mod call_grid;
pub mod corner_badge;
pub mod elapsed_time;
pub mod equal_width;
pub mod fills_parent;
pub mod fps;
pub mod help_mode;
pub mod link;
pub mod on_appear;
pub mod pan;
pub mod pie_progress;
pub mod polyicon;
pub mod profile;
pub mod right_click;
pub mod shader;
pub mod static_checkbox;
pub mod tile_background;
pub mod tooltip;
pub mod track_bounds;
pub mod track_scroll;
pub mod volume_bar;

pub use call_grid::call_grid;
pub use corner_badge::{CornerContent, corner_badge};
pub use elapsed_time::elapsed_time;
pub use fills_parent::fills_parent;
pub use on_appear::on_appear;
pub use pan::pan;
pub use volume_bar::volume_bar;

use crate::common::DePlaceWidget;

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

/// The UI font, bundled so regular text looks the same across platforms
/// instead of falling back to whatever system sans-serif font each OS ships.
pub const UI_FONT: Font = Font {
    family: Family::Name("Inter"),
    ..Font::DEFAULT
};

pub fn weighted_text<'a, T>(
    text: impl IntoFragment<'a>,
    weight: iced::font::Weight,
) -> Rich<'a, (), T> {
    rich_text([span(text).font(Font { weight, ..UI_FONT })])
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

pub fn text_input<T>(
    placeholder: &'static str,
    value: impl Into<String>,
    theme: Theme,
    structure: Structure,
) -> w::text_input::TextInput<'static, T>
where
    T: Clone + 'static,
{
    use w::text_input::{Status, Style};

    w::text_input(placeholder, value.into())
        .size(structure.font_size)
        .padding(structure.small_gap)
        .style(move |_theme, status| Style {
            background: theme.solid_bg.into(),
            border: Border {
                color: if matches!(status, Status::Focused { .. }) {
                    theme.accent.into()
                } else {
                    theme.border.into()
                },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            placeholder: theme.text.muted.into(),
            selection: theme.text.muted.into(),
            value: theme.text.normal.into(),
        })
}

pub struct InsetShadow {
    radius: f32,
    color: Color,
    depth: f32,
    layers: usize,
}

impl InsetShadow {
    pub fn new(radius: f32, color: impl Into<Color>, depth: f32, layers: usize) -> Self {
        Self {
            radius,
            color: color.into(),
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
            // Start slightly inside the container's own clipped edge so the stroke
            // (which straddles its path) never bleeds past the boundary and hits the
            // degenerate radius == size/2 corner-arc case, which showed up as flattened
            // poles on the outermost ring.
            let inset = 1.0 + t * self.depth;
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

pub trait MapElementExt<'a, Message, Theme, Renderer> {
    fn map_el<B>(self, f: impl Fn(Message) -> B + 'a) -> Element<'a, B, Theme, Renderer>
    where
        B: 'static;
}

impl<'a, Message, Theme, Renderer, W> MapElementExt<'a, Message, Theme, Renderer> for W
where
    Message: 'static,
    Theme: 'static,
    Renderer: iced::advanced::Renderer + 'static,
    W: Widget<Message, Theme, Renderer> + 'a,
{
    fn map_el<B>(self, f: impl Fn(Message) -> B + 'a) -> Element<'a, B, Theme, Renderer>
    where
        B: 'static,
    {
        self.map(f).boxed()
    }
}

pub trait IcedColorExt {
    fn scale_lightness(&self, factor: f32) -> Self;
}
impl IcedColorExt for iced::Color {
    fn scale_lightness(&self, factor: f32) -> Self {
        iced::Color::from_rgba(self.r * factor, self.g * factor, self.b * factor, self.a)
    }
}

pub struct PhosphorIcon {
    handle: svg::Handle,
    size: f32,
    color: Option<Color>, // explicit override; None = inherit
}

impl PhosphorIcon {
    pub fn new(svg_content: &'static str, size: f32) -> Self {
        Self {
            handle: svg::Handle::from_memory(svg_content.as_bytes()),
            size,
            color: None,
        }
    }

    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }
}

impl Meta for PhosphorIcon {}

impl<Message, Theme, R: SvgRenderer> Widget<Message, Theme, R> for PhosphorIcon {
    fn size(&self) -> Size<iced::Length> {
        Size::new(self.size.into(), self.size.into())
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        _renderer: &R,
        limits: &iced::advanced::layout::Limits,
    ) {
        tree.size = limits.resolve(self.size, self.size, Size::new(self.size, self.size));
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut R,
        _theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: layout::Layout,
        _cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        renderer.draw_svg(
            iced::advanced::svg::Svg::new(self.handle.clone())
                .color(self.color.unwrap_or(style.text_color)),
            layout.bounds(),
            *viewport,
        );
    }
}

impl<'a, Message, Theme, R> From<PhosphorIcon> for Element<'a, Message, Theme, R>
where
    Message: 'a,
    Theme: 'a,
    R: SvgRenderer + 'a,
{
    fn from(icon: PhosphorIcon) -> Self {
        icon.boxed()
    }
}

pub fn phosphor_icon(svg_content: &'static str, size: f32) -> PhosphorIcon {
    PhosphorIcon::new(svg_content, size)
}

pub trait IcedWidget<T, V> {
    fn update(&mut self, message: T) -> Option<V>;
    fn view(&self) -> iced::Element<'static, T>;
}

pub trait StatusExt {
    fn active(&self) -> bool;
}

impl StatusExt for w::button::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Hovered | Self::Pressed)
    }
}

impl StatusExt for w::text_input::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Focused { .. })
    }
}

impl StatusExt for w::text_editor::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Focused { .. })
    }
}

impl StatusExt for w::scrollable::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Dragged { .. })
    }
}

impl StatusExt for sweeten::widget::toggler::Status {
    fn active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }
}

pub fn blurhash_to_image(hash: &str) -> Option<ImageHandle> {
    let width = 32;
    let height = 32;

    let pixels = match blurhash::decode(hash, width, height, 1.2) {
        Ok(pixels) => pixels,
        Err(e) => {
            tracing::error!("Failed to decode blurhash: {:?}", e);
            return None;
        }
    };

    Some(ImageHandle::from_rgba(width, height, pixels))
}

pub fn thumbhash_to_image(hash: &Base64) -> Option<ImageHandle> {
    let (width, height, pixels) = match thumbhash::thumb_hash_to_rgba(hash.as_bytes()) {
        Ok(decoded) => decoded,
        Err(e) => {
            tracing::error!("Failed to decode thumbhash: {:?}", e);
            return None;
        }
    };

    Some(ImageHandle::from_rgba(width as u32, height as u32, pixels))
}

pub fn themed_scrollable<'a, T: 'a, W>(
    content: W,
    theme: Theme,
    structure: Structure,
) -> Scrollable<'a, T, W> {
    w::scrollable(content)
        .style(move |_, status| {
            let border = Border {
                color: theme.border.into(),
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            };

            let rail = w::scrollable::Rail {
                background: None,
                border,
                scroller: w::scrollable::Scroller {
                    background: theme.solid_hover_bg.into(),
                    border: border.color(if status.active() {
                        theme.accent
                    } else {
                        theme.border
                    }),
                },
            };

            w::scrollable::Style {
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: None,
                container: w::container::Style::default(),
                auto_scroll: w::scrollable::AutoScroll {
                    background: theme.solid_hover_bg.into(),
                    border,
                    shadow: Default::default(),
                    icon: theme.accent.into(),
                },
            }
        })
        .smooth_scroll(true)
        .auto_scroll(true)
}

pub trait CopyUserIdExt {
    fn copy_user_id(id: OwnedUserId) -> Self;
}

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

pub fn render_media_failed_to_load<'a, T: Clone + 'a>(
    theme: Theme,
    structure: Structure,
    width: f32,
    height: f32,
    kind: MediaType,
) -> Element<'a, T> {
    w::stack![
        w::container(
            weighted_text(
                format!("{} failed to load", kind.display_string()),
                Weight::Bold
            )
            .size(structure.chat.text_size * 1.5)
            .width(Fill)
            .height(Fill)
            .center(),
        )
        .width(width)
        .height(height)
        .style(move |_| w::container::Style {
            background: Some(theme.error_blended().into()),
            text_color: Some(theme.colors.error.into()),
            border: Border {
                color: theme.colors.error.into(),
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }),
        Canvas::new(InsetShadow::new(
            structure.inner_border_radius,
            theme.colors.error,
            structure.chat.text_size / 2.0,
            8,
        ))
        .width(width)
        .height(height),
    ]
    .boxed()
}

pub trait Explainable<'a, Message, Entity, Theme, Renderer>
where
    Message: 'a,
    Renderer: iced::advanced::Renderer,
    Self: Widget<Message, Theme, Renderer>,
{
    fn explain_white(self) -> impl Widget<Message, Theme, Renderer>;
}

impl<'a, Message, Entity, Theme, Renderer> Explainable<'a, Message, Entity, Theme, Renderer>
    for Entity
where
    Message: 'a,
    Renderer: iced::advanced::Renderer,
    Entity: Widget<Message, Theme, Renderer>,
{
    fn explain_white(self) -> impl Widget<Message, Theme, Renderer> {
        self.explain(Color::WHITE)
    }
}

pub fn close_button<'a, T: Clone + 'a>(
    theme: Theme,
    structure: Structure,
    on_press: T,
) -> Element<'a, T> {
    w::container(
        w::button(phosphor_icon(
            phosphor_svgs::icon::x::BOLD,
            structure.chat.text_size,
        ))
        .padding(structure.small_gap / 2.0)
        .style(move |_, status| w::button::Style {
            background: if status.active() {
                Some(theme.solid_hover_bg.into())
            } else {
                None
            },
            text_color: if status.active() {
                theme.text.normal.into()
            } else {
                theme.text.dim.into()
            },
            border: border::rounded(structure.semi_large_border_radius()),
            ..Default::default()
        })
        .on_press(on_press),
    )
    .padding(structure.small_gap / 2.0)
    .boxed()
}

pub trait ColoredRounded<'a, T: 'a>
where
    Self: Sized,
{
    fn colored_rounded(self, color: impl Into<Background>, rounded: impl Into<Radius>) -> Self;
    fn maybe_colored_rounded<C: Into<Background>>(
        self,
        color: Option<C>,
        rounded: impl Into<Radius>,
    ) -> Self;
}

impl<'a, T: 'a> ColoredRounded<'a, T> for Container<'a, T> {
    fn colored_rounded(self, color: impl Into<Background>, rounded: impl Into<Radius>) -> Self {
        let background = color.into();
        let rounded = rounded.into();
        self.style(move |_| w::container::Style {
            background: Some(background),
            border: border::rounded(rounded),
            ..Default::default()
        })
    }

    fn maybe_colored_rounded<C: Into<Background>>(
        self,
        color: Option<C>,
        rounded: impl Into<Radius>,
    ) -> Self {
        let rounded = rounded.into();
        let color = color.map(|c| c.into());
        self.style(move |_| w::container::Style {
            background: color,
            border: border::rounded(rounded),
            ..Default::default()
        })
    }
}

pub trait PushElement<'a, Message: 'a, Content: DePlaceWidget<Message> + 'a> {
    fn push_element(self, element: Content) -> Self;
}

impl<'a, Message: 'a, Content: DePlaceWidget<Message> + 'a> PushElement<'a, Message, Content>
    for Column<Element<'a, Message>>
{
    fn push_element(self, element: Content) -> Self {
        self.push(element.boxed())
    }
}

impl<'a, Message: 'a, Content: DePlaceWidget<Message> + 'a> PushElement<'a, Message, Content>
    for Row<Element<'a, Message>>
{
    fn push_element(self, element: Content) -> Self {
        self.push(element.boxed())
    }
}

impl<'a, Message: 'a, Content: DePlaceWidget<Message> + 'a> PushElement<'a, Message, Content>
    for Stack<Element<'a, Message>>
{
    fn push_element(self, element: Content) -> Self {
        self.push(element.boxed())
    }
}

pub fn color_stops<C: Into<Color>>(
    colors: impl IntoIterator<Item = (f32, C)>,
) -> impl IntoIterator<Item = ColorStop> {
    colors.into_iter().map(|(offset, color)| ColorStop {
        offset,
        color: color.into(),
    })
}
