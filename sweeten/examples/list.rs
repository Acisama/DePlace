use iced::widget::{
    Widget, button, center, column, container, lazy, row, scrollable, space,
    text,
};
use iced::{Center, Element, Fill, Theme};
use sweeten::widget::list;

pub fn main() -> iced::Result {
    iced::application(List::default, List::update, List::view)
        .theme(Theme::TokyoNight)
        .run()
}

struct List {
    content: list::Content<usize, State>,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Update(usize),
    Remove(usize),
}

impl List {
    fn update(&mut self, message: Message) {
        match message {
            Message::Update(index) => {
                if let Some(state) = self.content.get_index_mut(index) {
                    *state = State::Updated;
                }
            }
            Message::Remove(index) => {
                let _ = self.content.remove(index);
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        center(
            scrollable(container(list_view(self.content.clone())).padding(10))
                .width(Fill),
        )
        .padding(10)
        .boxed()
    }
}

/// Necessary because ice's view reqires '_ lifetime
fn list_view(
    content: list::Content<usize, State>,
) -> Element<'static, Message> {
    list(content, |index, key, value| {
        // Keyed the same way `keyed_column` rows are: hashing on `id`
        // (plus `state`, since that's the only other thing the row
        // depends on) means the row only re-renders when its own data
        // actually changes, not when some other item in the list is
        // added, updated, or removed.
        lazy(
            (key, value),
            move |&(id, ref state)| -> Element<'_, Message> {
                println!("rendering item {id}");
                row![
                    match **state {
                        State::Idle => {
                            text(format!("I am item {id}!")).boxed()
                        }
                        State::Updated => center(
                            column![
                                text(format!("I am item {id}!")),
                                text("... but different!")
                            ]
                            .spacing(20)
                        )
                        .height(300)
                        .boxed(),
                    },
                    space(),
                    button("Update").on_press_maybe(
                        matches!(**state, State::Idle)
                            .then_some(Message::Update(index))
                    ),
                    button("Remove")
                        .on_press(Message::Remove(index))
                        .style(button::danger)
                ]
                .spacing(10)
                .padding(5)
                .align_y(Center)
                .boxed()
            },
        )
        .boxed()
    })
    .boxed()
}

impl Default for List {
    fn default() -> Self {
        Self {
            content: list::Content::from_iter(
                (0..1_000).map(|id| (id, State::Idle)),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum State {
    Idle,
    Updated,
}
