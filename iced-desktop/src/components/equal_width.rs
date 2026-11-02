//! `EqualWidth`: a vertical stack whose width is the widest child's intrinsic
//! width, and which then gives *every* child exactly that width — so `Fill`
//! children stretch to the widest sibling instead of to the window edge.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{Meta, Operation, Tree};
use iced::advanced::{Shell, Widget};
use iced::{
    Element, Event, Length, Padding, Pixels, Rectangle, Renderer, Size, Vector, Widget as _, mouse,
};

use crate::common::DePlaceWidget;

pub struct EqualWidth<Message, Content>
where
    Content: DePlaceWidget<Message>,
{
    children: Vec<Content>,
    spacing: f32,
    padding: Padding,
    _marker: std::marker::PhantomData<Message>,
}

pub fn equal_width<Message, Content>(
    children: impl IntoIterator<Item = Content>,
) -> EqualWidth<Message, Content>
where
    Content: DePlaceWidget<Message>,
{
    EqualWidth {
        children: children.into_iter().collect(),
        spacing: 0.0,
        padding: Padding::ZERO,
        _marker: std::marker::PhantomData,
    }
}

impl<Message, Content: DePlaceWidget<Message>> EqualWidth<Message, Content> {
    pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
        self.spacing = amount.into().0;
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
}

impl<Message, Content: DePlaceWidget<Message>> Meta for EqualWidth<Message, Content> {}

impl<Message, Content: DePlaceWidget<Message>> Widget<Message, iced::Theme, Renderer>
    for EqualWidth<Message, Content>
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
            child.layout(state, renderer, &measure);
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
            child.layout(state, renderer, &pinned);
            state.translation = Vector::new(self.padding.left, self.padding.top + y);
            y += state.size.height;
        }

        tree.size = Size::new(widest, y).expand(self.padding);
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
        for (child, (layout, tree)) in self.children.iter().zip(layout.iter(&tree.children)) {
            child.draw(tree, renderer, theme, style, layout, cursor, viewport);
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
                child.operate(tree, layout, viewport, renderer, operation);
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
            child.update(tree, event, layout, cursor, renderer, shell, viewport);
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
                child.mouse_interaction(tree, layout, cursor, viewport, renderer)
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
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, Renderer>> {
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

impl<'a, Message, Content> From<EqualWidth<Message, Content>>
    for Element<'a, Message, iced::Theme, Renderer>
where
    Content: DePlaceWidget<Message> + 'a,
    Message: 'a,
{
    fn from(widget: EqualWidth<Message, Content>) -> Self {
        widget.boxed()
    }
}
