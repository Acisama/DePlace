use iced::advanced::widget::{Meta, Operation, Tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::LineHeight;
use iced::{Event, Length, Vector};

use crate::common::*;

use super::phosphor_icon;

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

pub struct CornerBadge<Content, Message>
where
    Content: DePlaceWidget<Message>,
{
    base: Content,
    ratio: f32,
    bg_circle_size: f32,
    background: Color,
    tl: Option<CornerContent>,
    tr: Option<CornerContent>,
    bl: Option<CornerContent>,
    br: Option<CornerContent>,
    // Rebuilt every `layout` call from `tl`/`tr`/`bl`/`br`, since the corner
    // circles' sizes depend on `base`'s measured size.
    overlay: Element<'static, Message>,
}

pub fn corner_badge<Content: DePlaceWidget<Message>, Message: 'static>(
    base: Content,
    ratio: f32,
    bg_circle_size: f32,
    background: impl Into<Color>,
) -> CornerBadge<Content, Message> {
    CornerBadge {
        base,
        ratio,
        bg_circle_size,
        background: background.into(),
        tl: None,
        tr: None,
        bl: None,
        br: None,
        overlay: EMPTY_WIDGET.boxed(),
    }
}

impl<Content: DePlaceWidget<Message>, Message> CornerBadge<Content, Message> {
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
            .size((diameter * 0.7).max(1.0))
            .line_height(LineHeight::Relative(1.0))
            .color(*color)
            .center()
            .width(Fill)
            .height(Fill)
            .boxed(),
        CornerContent::Icon {
            svg_content, color, ..
        } => phosphor_icon(svg_content, diameter * 0.8)
            .color(*color)
            .into(),
    }
}

pub fn notch_circle<'a, T: 'a>(diameter: f32, background: Color) -> Element<'a, T> {
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
        .boxed()
}

pub fn content_circle<'a, T: 'a>(content: &CornerContent, diameter: f32) -> Element<'a, T> {
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
        .boxed()
}

pub fn positioned<'a, T: 'a>(
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
        .boxed()
}

/// Builds the overlay of notch + content circles for whichever corners have
/// content, sized against `base_dim` (the shorter side of `base`'s measured
/// size).
fn build_overlay<Message: 'static>(
    base_dim: f32,
    ratio: f32,
    bg_circle_size: f32,
    background: Color,
    corners: [(&Option<CornerContent>, Horizontal, Vertical); 4],
) -> Element<'static, Message> {
    let notch_diameter = base_dim * (ratio + bg_circle_size);
    let content_diameter = base_dim * ratio;
    let overflow = base_dim * bg_circle_size / 2.0;

    let mut stack = Stack::new();
    for (content, h, v) in corners {
        if let Some(content) = content {
            let visible = content_circle(content, content_diameter);
            stack = stack.push(positioned(visible, h, v, 0.0));
            let notch = notch_circle(notch_diameter, background);
            stack = stack.push(positioned(notch, h, v, overflow));
            let visible = content_circle(content, content_diameter);
            stack = stack.push(positioned(visible, h, v, 0.0));
        }
    }

    stack.boxed()
}

impl<Content: DePlaceWidget<Message>, Message> Meta for CornerBadge<Content, Message> {}

impl<Content: DePlaceWidget<Message>, Message: 'static> Widget<Message, iced::Theme, Renderer>
    for CornerBadge<Content, Message>
{
    fn size(&self) -> Size<Length> {
        self.base.size()
    }

    fn diff(&mut self, tree: &mut Tree) {
        if tree.children.is_empty() {
            tree.children = vec![Tree::new(&self.base), Tree::new(&self.overlay)];
        }

        tree.children[0].diff(&mut self.base);
        // tree.children[1] (the overlay) is fully rebuilt in `layout` below,
        // since its content depends on the measured size.
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.base.layout(&mut tree.children[0], renderer, limits);
        let size = tree.children[0].size;

        self.overlay = build_overlay(
            size.width.min(size.height),
            self.ratio,
            self.bg_circle_size,
            self.background,
            [
                (&self.tl, Horizontal::Left, Vertical::Top),
                (&self.tr, Horizontal::Right, Vertical::Top),
                (&self.bl, Horizontal::Left, Vertical::Bottom),
                (&self.br, Horizontal::Right, Vertical::Bottom),
            ],
        );

        tree.children[1] = Tree::new(&self.overlay);
        self.overlay.diff(&mut tree.children[1]);
        self.overlay.layout(
            &mut tree.children[1],
            renderer,
            &layout::Limits::new(size, size),
        );
        tree.children[1].size = size;

        tree.size = size;
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.base.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        self.overlay.draw(
            &tree.children[1],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.base
            .operate(&mut tree.children[0], layout, viewport, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.base.update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.base
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        self.base.overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Content, Message> From<CornerBadge<Content, Message>> for Element<'a, Message>
where
    Message: 'static,
    Content: DePlaceWidget<Message> + 'a,
{
    fn from(badge: CornerBadge<Content, Message>) -> Self {
        badge.boxed()
    }
}
