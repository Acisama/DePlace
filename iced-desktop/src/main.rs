use deplace_core::{RestoreResult, state::AppState, try_restore};
use iced::{Task, window};
use matrix_sdk::Client;
use tracing_subscriber::EnvFilter;

use crate::components::Root;

mod components;

pub enum AppMessage {
    Start(window::Id),
    Restored(RestoreResult),
    GoToLoading,
    GoToDiscovery(Client),
    GoToLogin(Client),
    GoToHome { state: Box<AppState> },
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(
                "warn,desktop=trace,deplace_core=trace,matrix_sdk::http_client=off,zbus=error",
            )
        }))
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("Panic: {:?}", info);
        default_hook(info);
    }));

    iced::daemon(
        move || {
            let (_id, open_task) = window::open(window::Settings {
                maximized: true,
                ..Default::default()
            });

            let tasks = Task::batch([
                open_task.map(AppMessage::Start),
                Task::perform(try_restore(), AppMessage::Restored),
            ]);

            (Root::default(), tasks)
        },
        Root::update,
        Root::view,
    )
    .run()
}
