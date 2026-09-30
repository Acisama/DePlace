//! A wrapper widget that pans its content back and forth horizontally,
//! instead of letting it overflow, whenever it doesn't fit the width it's
//! given.
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, window};

use crate::components::animation_clock::elapsed_seconds;

/// How long content pauses at each end of the pan before reversing.
const PAUSE_SECONDS: f32 = 1.0;

pub struct Pan<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    width: Length,
    height: Length,
    speed: f32,
}

impl<'a, Message, Theme, Renderer> Pan<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            width: Length::Shrink,
            height: Length::Shrink,
            speed: 40.0,
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

    /// Pixels per second the content pans at, once it overflows its bounds.
    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }
}

/// Wraps `content` so that, whenever it doesn't fit the width it's given, it
/// pans back and forth horizontally instead of overflowing.
pub fn pan<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Pan<'a, Message, Theme, Renderer> {
    Pan::new(content)
}

/// How far the content should currently be shifted left, in `[0, overflow]`:
/// pauses at the left edge, pans to the right edge, pauses there, pans back.
///
/// Driven entirely by the shared [`elapsed_seconds`] clock rather than any
/// state of our own, so the widget stays stateless.
fn shift(overflow: f32, speed: f32) -> f32 {
    if overflow <= 0.0 || speed <= 0.0 {
        return 0.0;
    }

    let travel = overflow / speed;
    let cycle = 2.0 * PAUSE_SECONDS + 2.0 * travel;
    let t = elapsed_seconds() % cycle;

    if t < PAUSE_SECONDS {
        0.0
    } else if t < PAUSE_SECONDS + travel {
        (t - PAUSE_SECONDS) * speed
    } else if t < 2.0 * PAUSE_SECONDS + travel {
        overflow
    } else {
        overflow - (t - 2.0 * PAUSE_SECONDS - travel) * speed
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Pan<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let limits = limits.width(self.width).height(self.height);

        // Let the content measure its own natural width, uncapped by the
        // space we've actually been given - that gap is the overflow we pan.
        let unbounded = layout::Limits::with_flags(
            Size::new(0.0, limits.min.height),
            limits.max,
            Size::new(false, false),
            Size::new(true, false),
        );

        let child = &mut tree.children[0];
        self.content
            .as_widget_mut()
            .layout(child, renderer, &unbounded);

        tree.size = limits.resolve(self.width, self.height, child.size);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() else {
            return;
        };
        self.content
            .as_widget_mut()
            .operate(tree, layout, viewport, renderer, operation);
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
        let bounds = layout.bounds();
        let Some((content_layout, content_tree)) = layout.iter_mut(&mut tree.children).next()
        else {
            return;
        };
        let overflow = (content_layout.size().width - bounds.width).max(0.0);

        if overflow > 0.0 && matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
            // Keep the animation alive: schedule the next frame ourselves,
            // rather than depending on something else in the tree already
            // redrawing continuously.
            shell.request_redraw();
        }

        let translation = Vector::new(shift(overflow, self.speed), 0.0);

        let cursor = match cursor.position_over(bounds) {
            Some(position) => mouse::Cursor::Available(position + translation),
            None => cursor.obstruct() + translation,
        };

        self.content.as_widget_mut().update(
            content_tree,
            event,
            content_layout,
            cursor,
            renderer,
            shell,
            &(*viewport + translation),
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
        let bounds = layout.bounds();
        let Some((content_layout, content_tree)) = layout.iter(&tree.children).next() else {
            return mouse::Interaction::default();
        };
        let overflow = (content_layout.size().width - bounds.width).max(0.0);
        let translation = Vector::new(shift(overflow, self.speed), 0.0);

        let cursor = match cursor.position_over(bounds) {
            Some(position) => mouse::Cursor::Available(position + translation),
            None => cursor.obstruct() + translation,
        };

        self.content.as_widget().mouse_interaction(
            content_tree,
            content_layout,
            cursor,
            &(*viewport + translation),
            renderer,
        )
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
        let bounds = layout.bounds();
        let Some(clipped_viewport) = bounds.intersection(viewport) else {
            return;
        };

        let Some((content_layout, content_tree)) = layout.iter(&tree.children).next() else {
            return;
        };
        let overflow = (content_layout.size().width - bounds.width).max(0.0);
        let translation = Vector::new(shift(overflow, self.speed), 0.0);

        let cursor = match cursor.position_over(bounds) {
            Some(position) => mouse::Cursor::Available(position + translation),
            None => cursor.obstruct() + translation,
        };

        renderer.with_layer(clipped_viewport, |renderer| {
            renderer.with_translation(
                (-translation).hint(renderer.hint_factor().unwrap_or(1.0)),
                |renderer| {
                    self.content.as_widget().draw(
                        content_tree,
                        renderer,
                        theme,
                        style,
                        content_layout,
                        cursor,
                        &(clipped_viewport + translation),
                    );
                },
            );
        });
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
        let bounds = layout.bounds();
        let Some((content_layout, content_tree)) = layout.iter_mut(&mut tree.children).next()
        else {
            return Vec::new();
        };
        let overflow = (content_layout.size().width - bounds.width).max(0.0);
        let my_translation = Vector::new(shift(overflow, self.speed), 0.0);
        let viewport = viewport.intersection(&bounds).unwrap_or(*viewport);

        self.content.as_widget_mut().overlay(
            content_tree,
            content_layout,
            renderer,
            &(viewport + my_translation),
            translation - my_translation,
            window,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Pan<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(widget: Pan<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
