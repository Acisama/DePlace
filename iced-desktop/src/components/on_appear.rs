//! A wrapper widget that publishes a message once, the first time it is processed
//! by the runtime, instead of in response to user interaction.
use iced::advanced::widget::{Meta, Operation, Tree, tree};
use iced::advanced::{Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, advanced::Layout};
use iced::{Renderer, Widget as _};

use crate::common::DePlaceWidget;

/// A wrapper widget that publishes `Message` once, the first time it is processed
/// by the runtime, without waiting for user interaction.
///
/// This was created to be used in the timeline, so that an image or video is
/// only fetched by the client, when it is supposed to appear.
pub struct OnAppear<Message, Content: DePlaceWidget<Message>> {
    /// The content of the widget, that is dispalyed
    content: Content,
    /// The Message that is emitted exactly once, on the first time the widget is being processed by the runtime.
    message: Message,
}

impl<Message, Content: DePlaceWidget<Message>> OnAppear<Message, Content> {
    pub fn new(content: Content, message: Message) -> Self {
        Self { content, message }
    }
}

/// Creates an [`OnAppear`] that publishes `message` once, the first time it is
/// processed by the runtime, without waiting for any user interaction.
pub fn on_appear<Message, Content: DePlaceWidget<Message>>(
    content: Content,
    message: Message,
) -> OnAppear<Message, Content> {
    OnAppear::new(content, message)
}

#[derive(Default)]
struct State {
    fired: bool,
}

impl<Message, Content: DePlaceWidget<Message>> Meta for OnAppear<Message, Content> {}

impl<Message: Clone, Content: DePlaceWidget<Message>> Widget<Message, iced::Theme, Renderer>
    for OnAppear<Message, Content>
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
        self.content.size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content.layout(&mut tree.children[0], renderer, limits);

        // a widget's `layout` must record its resulting
        // size on its tree, the virtualized list measures rows by
        // reading `tree.size` after laying them out. Wrapping rows in
        // on_appear made them measure as zero-sized and render as empty.
        // Silly me, I should totally have known that. Totally.
        tree.size = tree.children[0].size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
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
        self.content.update(
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
        self.content
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
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
        self.content.draw(
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
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, Renderer>> {
        self.content.overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Message, Content> From<OnAppear<Message, Content>>
    for Element<'a, Message, iced::Theme, Renderer>
where
    Content: DePlaceWidget<Message> + 'a,
    Message: 'a + Clone,
{
    fn from(widget: OnAppear<Message, Content>) -> Self {
        widget.boxed()
    }
}
