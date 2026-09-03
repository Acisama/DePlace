use deplace_core::{RestoreResult, try_restore};
use iced::{Task, window};
use matrix_sdk::Client;
use tracing_subscriber::EnvFilter;

use crate::components::{
    authentification::{
        discovery::DiscoveryMessage, login::LoginMessage, verification::VerificationMessage,
    },
    home::HomeMessage,
    root::Root,
};

pub(crate) mod common;
pub(crate) mod components;
mod things;

#[derive(Debug, Clone)]
pub enum AppMessage {
    None,
    KeyboardEvent(iced::keyboard::Event),
    Start(window::Id),
    Discovery(DiscoveryMessage),
    Login(LoginMessage),
    Verification(VerificationMessage),
    Home(HomeMessage),
    Restored(RestoreResult),
    TabPressed { shift: bool },
    GoToLoading,
    GoToLogin(Client),
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(
                "warn,iced_desktop=trace,deplace_core=trace,matrix_sdk::http_client=off,zbus=error",
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

    let icon = match iced::window::icon::from_file_data(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/deplace_icon.png"
        )),
        Some(image::ImageFormat::Png),
    ) {
        Ok(icon) => icon,
        Err(e) => {
            tracing::error!("Failed to load icon: {:?}", e);
            return Err(iced::Error::WindowCreationFailed(Box::new(e)));
        }
    };

    iced::daemon(
        move || {
            let (_id, open_task) = window::open(window::Settings {
                maximized: true,
                icon: Some(icon.clone()),
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
    .title(Root::title)
    .subscription(Root::subscription)
    // Use monospace in specific things, not everywhere
    // .default_font(iced::Font::MONOSPACE)
    .run()
}
