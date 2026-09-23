//! A wrapper widget that publishes a message once, the first time it is processed
//! by the runtime, instead of in response to user interaction.
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, advanced::Layout};

/// A wrapper widget that publishes `Message` once, the first time it is processed
/// by the runtime, without waiting for user interaction.
///
/// This was created to be used in the timeline, so that an image or video is
/// only fetched by the client, when it is supposed to appear.
pub struct OnAppear<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    /// The content of the widget, that is dispalyed
    content: Element<'a, Message, Theme, Renderer>,
    /// The Message that is emitted exactly once, on the first time the widget is being processed by the runtime.
    message: Message,
}

impl<'a, Message, Theme, Renderer> OnAppear<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        message: Message,
    ) -> Self {
        Self {
            content: content.into(),
            message,
        }
    }
}

/// Creates an [`OnAppear`] that publishes `message` once, the first time it is
/// processed by the runtime, without waiting for any user interaction.
pub fn on_appear<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    message: Message,
) -> OnAppear<'a, Message, Theme, Renderer> {
    OnAppear::new(content, message)
}

#[derive(Default)]
struct State {
    fired: bool,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for OnAppear<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.content]);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            viewport,
            renderer,
            operation,
        );
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        // Own state, not the child's - tracks whether we've already published
        // our one-shot message, so it survives across `view()` rebuilds even
        // though `self.message` is fresh (and un-fired) every time.
        let state: &mut State = tree.state.downcast_mut();

        if !state.fired {
            state.fired = true;
            shell.publish(self.message.clone());
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
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
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<OnAppear<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(widget: OnAppear<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
