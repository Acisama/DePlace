#![recursion_limit = "256"]
#![windows_subsystem = "windows"]

use std::{
    any::TypeId,
    fmt::Debug,
    hash::Hash,
    io::{Read, Write},
    sync::Arc,
};

use deplace_core::{
    APP_HUMAN_NAME, APP_NAME, RestoreResult, SOCKET_NAME, state::ImportantPaths, try_restore,
};
use iced::{Task, advanced::subscription::Recipe, futures::stream, window};
use interprocess::local_socket::{
    GenericNamespaced, Listener, ListenerNonblockingMode, ListenerOptions, prelude::*,
};
use matrix_sdk::Client;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
    menu::{
        Menu, MenuEvent, MenuItem, PredefinedMenuItem,
        accelerator::{Accelerator, Code, Modifiers},
    },
};

use crate::components::{
    authentification::{
        discovery::DiscoveryMessage, login::LoginMessage, verification::VerificationMessage,
    },
    home::HomeMessage,
    root::Root,
};

pub mod common;
pub mod components;

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
    Tray(TrayAction),
}

#[derive(Debug, Clone)]
pub enum TrayAction {
    Open,
    CloseWindow,
    Exit,
}

const TRAY_MENU_OPEN_ID: &str = "deplace-tray-open";
const TRAY_MENU_CLOSE_ID: &str = "deplace-tray-close";
const TRAY_MENU_EXIT_ID: &str = "deplace-tray-exit";

const ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../assets/deplace_icon.png"
));

fn load_tray_icon() -> Result<tray_icon::Icon, iced::Error> {
    let image = image::load_from_memory_with_format(crate::ICON_PNG, image::ImageFormat::Png)
        .map_err(|e| iced::Error::WindowCreationFailed(Box::new(e)))?
        .resize(64, 64, image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    let (w, h) = image.dimensions();
    tray_icon::Icon::from_rgba(image.into_raw(), w, h)
        .map_err(|e| iced::Error::WindowCreationFailed(Box::new(e)))
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

    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    let file_layer = fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "warn,iced_desktop=trace,deplace_core=trace,matrix_sdk::http_client=off,zbus=error",
        )
    });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
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
    if let Err(e) = listener.set_nonblocking(ListenerNonblockingMode::Accept) {
        tracing::warn!("Failed to set local socket listener non-blocking: {e}");
    }

    let icon = match iced::window::icon::from_file_data(ICON_PNG, Some(image::ImageFormat::Png)) {
        Ok(icon) => icon,
        Err(e) => {
            tracing::error!("Failed to load icon: {:?}", e);
            return Err(iced::Error::WindowCreationFailed(Box::new(e)));
        }
    };

    let open = MenuItem::with_id(
        TRAY_MENU_OPEN_ID,
        "Open DePlace",
        true,
        Some(Accelerator::new(Modifiers::empty(), Code::KeyO)),
    );
    let close = MenuItem::with_id(
        TRAY_MENU_CLOSE_ID,
        "Close DePlace",
        true,
        Some(Accelerator::new(Modifiers::empty(), Code::KeyC)),
    );
    let exit = MenuItem::with_id(
        TRAY_MENU_EXIT_ID,
        "Exit DePlace",
        true,
        Some(Accelerator::new(Modifiers::empty(), Code::KeyQ)),
    );
    let menu = match Menu::with_items(&[&open, &close, &PredefinedMenuItem::separator(), &exit]) {
        Ok(menu) => menu,
        Err(e) => {
            tracing::error!("Failed to create system try menu: {e}");
            return Err(iced::Error::WindowCreationFailed(Box::new(e)));
        }
    };

    let tray_icon = load_tray_icon()?;

    let builder = TrayIconBuilder::new()
        .with_id(APP_NAME)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .with_title(APP_HUMAN_NAME)
        .with_tooltip(APP_HUMAN_NAME)
        .with_icon(tray_icon);

    let _tray = match builder.build() {
        Ok(tray) => tray,
        Err(e) => {
            tracing::warn!("Failed to build tray icon: {e}");
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

pub struct TrayMenuListener;

impl Recipe for TrayMenuListener {
    type Output = TrayAction;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        TypeId::of::<Self>().hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: iced::advanced::subscription::EventStream,
    ) -> iced::advanced::graphics::futures::BoxStream<Self::Output> {
        Box::pin(stream::unfold((), |_| async move {
            loop {
                let action = tokio::task::spawn_blocking(|| {
                    MenuEvent::receiver()
                        .recv_timeout(std::time::Duration::from_millis(250))
                        .ok()
                        .and_then(|event| match event.id().as_ref() {
                            TRAY_MENU_OPEN_ID => Some(TrayAction::Open),
                            TRAY_MENU_CLOSE_ID => Some(TrayAction::CloseWindow),
                            TRAY_MENU_EXIT_ID => Some(TrayAction::Exit),
                            _ => None,
                        })
                })
                .await
                .ok()?;

                if let Some(action) = action {
                    return Some((action, ()));
                }
            }
        }))
    }
}

pub struct TrayIconClickListener;

impl Recipe for TrayIconClickListener {
    type Output = TrayAction;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        TypeId::of::<Self>().hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: iced::advanced::subscription::EventStream,
    ) -> iced::advanced::graphics::futures::BoxStream<Self::Output> {
        Box::pin(stream::unfold((), |_| async move {
            loop {
                let action = tokio::task::spawn_blocking(|| {
                    TrayIconEvent::receiver()
                        .recv_timeout(std::time::Duration::from_millis(250))
                        .ok()
                        .and_then(|event| match event {
                            TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            } => Some(TrayAction::Open),
                            _ => None,
                        })
                })
                .await
                .ok()?;

                if let Some(action) = action {
                    return Some((action, ()));
                }
            }
        }))
    }
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
                    let msg = match listener_clone.incoming().next() {
                        Some(Ok(mut stream)) => {
                            let mut buf = [0u8; 5];
                            if stream.read_exact(&mut buf).is_ok() && &buf == b"focus" {
                                Some(FocusRequest)
                            } else {
                                None
                            }
                        }
                        Some(Err(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            None
                        }
                        _ => None,
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
