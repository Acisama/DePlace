use iced::{Element, Task, window};

struct AppState;

enum AppMessage {
    Start(window::Id),
}

fn main() -> iced::Result {
    iced::daemon(
        || {
            let (_id, open_task) = window::open(window::Settings::default());
            (AppState, open_task.map(AppMessage::Start))
        },
        AppState::update,
        AppState::view,
    )
    .run()
}

impl AppState {
    fn update(&mut self, _message: AppMessage) -> Task<AppMessage> {
        Task::none()
    }
    fn view(&self, _window: window::Id) -> Element<AppMessage> {
        "hello".into()
    }
}
