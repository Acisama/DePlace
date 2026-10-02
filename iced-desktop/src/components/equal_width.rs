//! `EqualWidth`: a vertical stack whose width is the widest child's intrinsic
//! width, and which then gives *every* child exactly that width — so `Fill`
//! children stretch to the widest sibling instead of to the window edge.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Shell, Widget};
use iced::{Element, Event, Length, Padding, Pixels, Rectangle, Size, Vector, mouse};

pub struct EqualWidth<'a, Message, Theme, Renderer> {
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    spacing: f32,
    padding: Padding,
}

pub fn equal_width<'a, Message, Theme, Renderer>(
    children: impl IntoIterator<Item = Element<'a, Message, Theme, Renderer>>,
) -> EqualWidth<'a, Message, Theme, Renderer> {
    EqualWidth {
        children: children.into_iter().collect(),
        spacing: 0.0,
        padding: Padding::ZERO,
    }
}

impl<'a, Message, Theme, Renderer> EqualWidth<'a, Message, Theme, Renderer> {
    pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
        self.spacing = amount.into().0;
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for EqualWidth<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        // Explicit Shrink: unlike `Fit`, it never merges with the
        // children's `Fill` and so never turns into `Fill` itself.
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let limits = limits.shrink(self.padding);
        let max = limits.max;

        // Pass 1: measure. With compression on, `Fill` resolves to the
        // child's intrinsic width instead of the full available width.
        let measure = layout::Limits::with_flags(
            Size::ZERO,
            max,
            Size::new(true, false),
            Size::new(false, false),
        );

        let mut widest = 0.0f32;
        for (child, state) in self.children.iter_mut().zip(&mut tree.children) {
            child.as_widget_mut().layout(state, renderer, &measure);
            widest = widest.max(state.size.width);
        }
        let widest = widest.min(max.width);

        // Pass 2: lay out every child with its width pinned to `widest`.
        // `Fill` takes the max, intrinsic widths get raised to the min.
        let pinned = layout::Limits::new(Size::new(widest, 0.0), Size::new(widest, max.height));

        let mut y = 0.0;
        for (i, (child, state)) in self.children.iter_mut().zip(&mut tree.children).enumerate() {
            if i > 0 {
                y += self.spacing;
            }
            child.as_widget_mut().layout(state, renderer, &pinned);
            state.translation = Vector::new(self.padding.left, self.padding.top + y);
            y += state.size.height;
        }

        tree.size = Size::new(widest, y).expand(self.padding);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for (child, (layout, tree)) in self.children.iter().zip(layout.iter(&tree.children)) {
            child
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds(), viewport);
        operation.traverse(&mut |operation| {
            for (child, (layout, tree)) in self
                .children
                .iter_mut()
                .zip(layout.iter_mut(&mut tree.children))
            {
                child
                    .as_widget_mut()
                    .operate(tree, layout, viewport, renderer, operation);
            }
        });
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
        for (child, (layout, tree)) in self
            .children
            .iter_mut()
            .zip(layout.iter_mut(&mut tree.children))
        {
            child
                .as_widget_mut()
                .update(tree, event, layout, cursor, renderer, shell, viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(layout.iter(&tree.children))
            .map(|(child, (layout, tree))| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<EqualWidth<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(widget: EqualWidth<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
