use std::f32::consts::TAU;
use std::time::Duration;

use iced::advanced::widget::{Meta, Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::container;
use iced::{Length, Vector, alignment, window};

use crate::common::*;

/// A [`container`](iced::widget::container) whose background/border/shadow
/// are recomputed every frame from a `pulse` in `0.0..=1.0` that eases like
/// a sine wave over `period`, instead of a fixed [`Theme`]/[`Status`] --
/// e.g. for loading-skeleton placeholders that should visibly breathe
/// without needing to hold any animation state of their own.
pub struct AnimatedBackground<Message, Content: DePlaceWidget<Message>> {
    content: Content,
    width: Length,
    height: Length,
    padding: Padding,
    period: Duration,
    style: Box<dyn Fn(f32) -> ContainerStyle>,
    _marker: std::marker::PhantomData<Message>,
}

impl<Message, Content: DePlaceWidget<Message>> AnimatedBackground<Message, Content> {
    pub fn new(content: Content) -> Self {
        Self {
            content,
            width: Length::Shrink,
            height: Length::Shrink,
            padding: Padding::ZERO,
            period: Duration::from_millis(1400),
            style: Box::new(|_| ContainerStyle::default()),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// How long one full pulse (dim -> bright -> dim) takes.
    pub fn period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }

    /// `style` is sampled fresh every frame with the current `pulse`.
    #[must_use]
    pub fn style(mut self, style: impl Fn(f32) -> ContainerStyle + 'static) -> Self {
        self.style = Box::new(style);
        self
    }
}

impl<Message, Content: DePlaceWidget<Message>> Meta for AnimatedBackground<Message, Content> {}

impl<Message, Content> Widget<Message, IcedTheme, Renderer> for AnimatedBackground<Message, Content>
where
    Content: DePlaceWidget<Message>,
{
    fn tag(&self) -> tree::Tag {
        self.content.tag()
    }

    fn state(&self) -> tree::State {
        self.content.state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));

        let size = self.content.size();
        self.width = self.width.stack(size.width);
        self.height = self.height.stack(size.height);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        container::layout(
            tree,
            limits,
            self.width,
            self.height,
            self.padding,
            alignment::Horizontal::Left,
            alignment::Vertical::Top,
            |tree, limits| {
                self.content.layout(tree, renderer, limits);
                tree.size
            },
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
        operation.container(None, layout.bounds(), viewport);
        operation.traverse(&mut |operation| {
            let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() else {
                return;
            };

            self.content
                .operate(tree, layout, viewport, renderer, operation);
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some((content_layout, content_tree)) = layout.iter_mut(&mut tree.children).next() {
            self.content.update(
                content_tree,
                event,
                content_layout,
                cursor,
                renderer,
                shell,
                viewport,
            );
        }

        // Keeps the clock-driven style ticking every frame for as long as
        // this widget stays on screen.
        if let iced::Event::Window(window::Event::RedrawRequested(_)) = event {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &IcedTheme,
        renderer_style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let style = (self.style)(pulse(self.period));

        container::draw_background(renderer, &style, bounds);

        let Some((content_layout, content_tree)) = layout.iter(&tree.children).next() else {
            return;
        };

        self.content.draw(
            content_tree,
            renderer,
            theme,
            &renderer::Style {
                text_color: style.text_color.unwrap_or(renderer_style.text_color),
            },
            content_layout,
            cursor,
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
        let Some((content_layout, content_tree)) = layout.iter(&tree.children).next() else {
            return mouse::Interaction::default();
        };

        self.content
            .mouse_interaction(content_tree, content_layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, IcedTheme, Renderer>> {
        let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() else {
            return vec![];
        };

        self.content
            .overlay(tree, layout, renderer, viewport, translation, window)
    }
}

/// A value in `0.0..=1.0` that eases like a sine wave once every `period`,
/// read from the app's shared animation clock -- so multiple
/// [`AnimatedBackground`]s with the same `period` pulse in lockstep instead
/// of drifting out of phase.
fn pulse(period: Duration) -> f32 {
    let t = super::animation_clock::elapsed_seconds() / period.as_secs_f32();
    (t * TAU).sin() * 0.5 + 0.5
}

impl<'a, Message, Content> From<AnimatedBackground<Message, Content>> for Element<'a, Message>
where
    Message: 'a,
    Content: DePlaceWidget<Message> + 'a,
{
    fn from(widget: AnimatedBackground<Message, Content>) -> Self {
        widget.boxed()
    }
}

pub fn animated_background<Message, Content>(
    content: Content,
) -> AnimatedBackground<Message, Content>
where
    Content: DePlaceWidget<Message>,
{
    AnimatedBackground::new(content)
}
