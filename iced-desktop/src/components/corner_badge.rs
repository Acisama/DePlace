use iced::alignment::{Horizontal, Vertical};
use iced::widget::responsive;

use crate::common::*;

#[derive(Clone)]
pub enum CornerContent {
    Text {
        text: String,
        background: Color,
        color: Color,
    },
    Icon {
        svg_content: &'static str,
        color: Color,
        background: Color,
    },
}

impl CornerContent {
    pub fn text(
        text: impl Into<String>,
        color: impl Into<Color>,
        background: impl Into<Color>,
    ) -> Self {
        Self::Text {
            text: text.into(),
            color: color.into(),
            background: background.into(),
        }
    }

    pub fn icon(
        svg_content: &'static str,
        color: impl Into<Color>,
        background: impl Into<Color>,
    ) -> Self {
        Self::Icon {
            svg_content,
            color: color.into(),
            background: background.into(),
        }
    }

    fn background(&self) -> Color {
        match self {
            Self::Text { background, .. } | Self::Icon { background, .. } => *background,
        }
    }
}

pub struct CornerBadge<'a, T> {
    base: Element<'a, T>,
    ratio: f32,
    bg_circle_size: f32,
    background: Color,
    tl: Option<CornerContent>,
    tr: Option<CornerContent>,
    bl: Option<CornerContent>,
    br: Option<CornerContent>,
}

pub fn corner_badge<'a, T: 'a>(
    base: impl Into<Element<'a, T>>,
    ratio: f32,
    bg_circle_size: f32,
    background: impl Into<Color>,
) -> CornerBadge<'a, T> {
    CornerBadge {
        base: base.into(),
        ratio,
        bg_circle_size,
        background: background.into(),
        tl: None,
        tr: None,
        bl: None,
        br: None,
    }
}

impl<'a, T: 'a> CornerBadge<'a, T> {
    pub fn tl(mut self, content: CornerContent) -> Self {
        self.tl = Some(content);
        self
    }

    pub fn tr(mut self, content: CornerContent) -> Self {
        self.tr = Some(content);
        self
    }

    pub fn bl(mut self, content: CornerContent) -> Self {
        self.bl = Some(content);
        self
    }

    pub fn br(mut self, content: CornerContent) -> Self {
        self.br = Some(content);
        self
    }
}

fn corner_glyph<'a, T: 'a>(content: &CornerContent, diameter: f32) -> Element<'a, T> {
    match content {
        CornerContent::Text { text, color, .. } => weighted_text(text.clone(), Weight::ExtraBold)
            .size(diameter * 0.7)
            .color(*color)
            .center()
            .width(Fill)
            .height(Fill)
            .into(),
        CornerContent::Icon {
            svg_content, color, ..
        } => phosphor_icon(svg_content, diameter * 0.8)
            .color(*color)
            .into(),
    }
}

fn notch_circle<'a, T: 'a>(diameter: f32, background: Color) -> Element<'a, T> {
    w::container(Space::new())
        .width(diameter)
        .height(diameter)
        .style(move |_| ContainerStyle {
            background: Some(background.into()),
            border: Border {
                radius: (diameter / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn content_circle<'a, T: 'a>(content: &CornerContent, diameter: f32) -> Element<'a, T> {
    let background_color = content.background();

    w::container(corner_glyph(content, diameter))
        .width(diameter)
        .height(diameter)
        .center(diameter)
        .style(move |_| ContainerStyle {
            background: Some(background_color.into()),
            border: Border {
                radius: (diameter / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn positioned<'a, T: 'a>(
    corner: Element<'a, T>,
    h: Horizontal,
    v: Vertical,
    overflow: f32,
) -> Element<'a, T> {
    let padding = Padding {
        top: if v == Vertical::Top { -overflow } else { 0.0 },
        bottom: if v == Vertical::Bottom {
            -overflow
        } else {
            0.0
        },
        left: if h == Horizontal::Left {
            -overflow
        } else {
            0.0
        },
        right: if h == Horizontal::Right {
            -overflow
        } else {
            0.0
        },
    };

    w::container(corner)
        .width(Fill)
        .height(Fill)
        .align_x(h)
        .align_y(v)
        .padding(padding)
        .into()
}

impl<'a, T: 'a> From<CornerBadge<'a, T>> for Element<'a, T> {
    fn from(badge: CornerBadge<'a, T>) -> Self {
        let CornerBadge {
            base,
            ratio,
            bg_circle_size,
            background,
            tl,
            tr,
            bl,
            br,
        } = badge;

        Stack::new()
            .push(base)
            .push(responsive(move |size| {
                let base_dim = size.width.min(size.height);
                let notch_diameter = base_dim * (ratio + bg_circle_size);
                let content_diameter = base_dim * ratio;
                let overflow = base_dim * bg_circle_size / 2.0;

                let corners: [(&Option<CornerContent>, Horizontal, Vertical); 4] = [
                    (&tl, Horizontal::Left, Vertical::Top),
                    (&tr, Horizontal::Right, Vertical::Top),
                    (&bl, Horizontal::Left, Vertical::Bottom),
                    (&br, Horizontal::Right, Vertical::Bottom),
                ];

                let mut overlay = Stack::new();
                for (content, h, v) in corners {
                    if let Some(content) = content {
                        let notch = notch_circle(notch_diameter, background);
                        overlay = overlay.push(positioned(notch, h, v, overflow));

                        let visible = content_circle(content, content_diameter);
                        overlay = overlay.push(positioned(visible, h, v, 0.0));
                    }
                }

                overlay
            }))
            .into()
    }
}
