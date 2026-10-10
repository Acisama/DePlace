#![recursion_limit = "256"]
#![windows_subsystem = "windows"]

use std::{
    any::TypeId,
    fmt::Debug,
    hash::Hash,
    io::{Read, Write},
    sync::Arc,
};

use deplace_core::{APP_NAME, RestoreResult, state::ImportantPaths, try_restore};
use iced::{Task, advanced::subscription::Recipe, futures::stream, window};
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, prelude::*};
use matrix_sdk::Client;
use tracing_subscriber::{EnvFilter, fmt::writer::MakeWriterExt};

use crate::components::{
    authentification::{
        discovery::DiscoveryMessage, login::LoginMessage, verification::VerificationMessage,
    },
    home::HomeMessage,
    root::Root,
};

pub mod common;
pub mod components;

const SOCKET_NAME: &str = "deplace_app_single_instance.sock";

#[derive(Debug, Clone)]
pub enum AppMessage {
    DoNothing,
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
    WindowClosed(window::Id),
    WindowOpened(window::Id),
    FocusRequest(FocusRequest),
    WindowFocus { focused: bool },
}

fn main() -> iced::Result {
    let paths = match ImportantPaths::new() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to create important paths: {e}");
            return Err(iced::Error::WindowCreationFailed(e.into()));
        }
    };

    let file_appender = tracing_appender::rolling::never(
        &paths.log_dir,
        format!("{}.log", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")),
    );
    let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);

    let multi_writer = std::io::stdout.and(non_blocking_file);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(
                "warn,iced_desktop=trace,deplace_core=trace,matrix_sdk::http_client=off,zbus=error",
            )
        }))
        .with_writer(multi_writer)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_ansi(false)
        .init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("Panic: {:?}", info);
        default_hook(info);
    }));

    // Convert socket identifier for interprocess
    let socket_name = match SOCKET_NAME.to_ns_name::<GenericNamespaced>() {
        Ok(name) => name,
        Err(e) => {
            tracing::error!("Failed to create socket name: {e}");
            return Err(iced::Error::WindowCreationFailed(Box::new(e)));
        }
    };

    // Try connecting to an existing instance
    if let Ok(mut stream) = LocalSocketStream::connect(socket_name.clone()) {
        tracing::info!("Another instance is already running. Sending focus signal.");
        if let Err(e) = stream.write_all(b"focus") {
            tracing::error!("Failed to send focus signal: {e}");
        }
        if let Err(e) = stream.flush() {
            tracing::error!("Failed to flush focus signal: {e}");
        }
        // Exit this secondary instance immediately
        return Ok(());
    }
    let listener = match ListenerOptions::new().name(socket_name).create_sync() {
        Ok(l) => Arc::new(l),
        Err(e) => {
            tracing::error!("Failed to bind local socket: {e}");
            return Err(iced::Error::WindowCreationFailed(Box::new(e)));
        }
    };

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
                #[cfg(target_os = "linux")]
                platform_specific: window::settings::PlatformSpecific {
                    application_id: APP_NAME.to_string(),
                    ..Default::default()
                },
                #[cfg(not(target_os = "linux"))]
                platform_specific: Default::default(),
                ..Default::default()
            });

            let tasks = Task::batch([
                open_task.map(AppMessage::Start),
                Task::perform(try_restore(paths.clone()), AppMessage::Restored),
            ]);

            (Root::new(paths.clone(), listener.clone()), tasks)
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

struct SocketListener {
    listener: Arc<Listener>,
}

#[derive(Debug, Clone)]
pub struct FocusRequest;

impl Recipe for SocketListener {
    type Output = FocusRequest;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        TypeId::of::<Self>().hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: iced::advanced::subscription::EventStream,
    ) -> iced::advanced::graphics::futures::BoxStream<Self::Output> {
        Box::pin(stream::unfold(self.listener, |listener| async move {
            loop {
                let listener_clone = listener.clone();

                let (msg, returned_listener) = tokio::task::spawn_blocking(move || {
                    let msg = {
                        if let Some(Ok(mut stream)) = listener_clone.incoming().next() {
                            let mut buf = [0u8; 5];
                            if stream.read_exact(&mut buf).is_ok() && &buf == b"focus" {
                                Some(FocusRequest)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    (msg, listener_clone)
                })
                .await
                .ok()?;

                if let Some(focus_request) = msg {
                    return Some((focus_request, returned_listener));
                }
            }
        }))
    }
}
