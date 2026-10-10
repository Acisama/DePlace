use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{
            Operation, Tree,
            operation::{self, Outcome, Scrollable as ScrollableOperationState},
            tree,
        },
    },
    widget::Id,
    window,
};

struct FindScrolledFromTop {
    target: Id,
    value: Option<f32>,
}

impl Operation<f32> for FindScrolledFromTop {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<f32>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _bounds: Rectangle,
        _content: Size,
        translation: Vector,
        _state: &mut dyn ScrollableOperationState,
    ) {
        if id == Some(&self.target) {
            self.value = Some(translation.y);
        }
    }

    fn finish(&self) -> Outcome<f32> {
        match self.value {
            Some(v) => Outcome::Some(v),
            None => Outcome::None,
        }
    }
}

pub struct TrackScroll<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    scrollable_id: iced::widget::Id,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
}

pub fn track_scroll<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    scrollable_id: iced::widget::Id,
    on_change: impl Fn(f32) -> Message + 'a,
) -> TrackScroll<'a, Message, Theme, Renderer> {
    TrackScroll {
        content: content.into(),
        scrollable_id,
        on_change: Box::new(on_change),
    }
}

#[derive(Default)]
struct State {
    last_value: Option<f32>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TrackScroll<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);

        tree.size = tree.children[0].size;
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

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let mut operation = FindScrolledFromTop {
                target: self.scrollable_id.clone(),
                value: None,
            };
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                viewport,
                renderer,
                &mut operation::black_box(&mut operation),
            );

            if let Some(value) = operation.value {
                let state: &mut State = tree.state.downcast_mut();
                if state.last_value != Some(value) {
                    state.last_value = Some(value);
                    shell.publish((self.on_change)(value));
                }
            }
        }
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

impl<'a, Message, Theme, Renderer> From<TrackScroll<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(widget: TrackScroll<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
