#![recursion_limit = "256"]

use deplace_core::APP_HUMAN_NAME;
use gpui::{App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*};
use gpui_component::Root;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::tokio::Listener as TokioListener;
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use assets::AppAssets;
use components::root::RootView;
use keybinds::load_keymap_from_json;
use theme::AppTheme;

use crate::theme::Structure;

const SOCKET_NAME: &str = "deplace.sock";

pub(crate) mod assets;
pub(crate) mod attachments;
pub(crate) mod cache;
pub(crate) mod components;
pub(crate) mod helpers;
pub(crate) mod keybinds;
pub(crate) mod saving;
pub(crate) mod theme;
pub(crate) mod view_lru;
pub(crate) mod watch_bridge;

#[derive(Serialize, Deserialize)]
enum InstanceCommand {
    FocusWindow,
}

#[derive(Default)]
pub enum GenericState {
    Success,
    #[default]
    Default,
    Disabled,
    Loading,
    Error(String),
}

impl GenericState {
    pub fn is_loading(&self) -> bool {
        matches!(self, GenericState::Loading)
    }

    pub fn is_disabled(&self) -> bool {
        matches!(
            self,
            GenericState::Error(_) | GenericState::Loading | GenericState::Disabled
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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

    let namespaced_socket_name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;

    // Another instance already holds the socket: ask it to focus its window and exit.
    if let Ok(mut stream) = LocalSocketStream::connect(namespaced_socket_name.clone()) {
        tracing::info!("Another instance is already running, focusing it");
        let msg = serde_json::to_vec(&InstanceCommand::FocusWindow)?;
        stream.write_all(&msg)?;
        stream.flush()?;
        return Ok(());
    }

    // Nobody's holding the socket, so we are the instance: keep it and run the UI directly
    let listener = ListenerOptions::new()
        .name(namespaced_socket_name)
        .create_tokio()?;

    let (focus_tx, focus_rx) = mpsc::unbounded_channel();
    tokio::spawn(listen_for_focus_requests(listener, focus_tx));

    run_ui(focus_rx);

    Ok(())
}

async fn listen_for_focus_requests(listener: TokioListener, focus_tx: mpsc::UnboundedSender<()>) {
    loop {
        let stream = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Couldn't accept IPC connection: {e}");
                continue;
            }
        };

        let focus_tx = focus_tx.clone();
        tokio::spawn(async move {
            let (mut reader, _) = stream.split();
            let mut buf = [0u8; 256];
            if let Ok(bytes_read) = reader.read(&mut buf).await
                && serde_json::from_slice::<InstanceCommand>(&buf[..bytes_read]).is_ok()
            {
                let _ = focus_tx.send(());
            }
        });
    }
}

fn run_ui(_: mpsc::UnboundedReceiver<()>) {
    let tokio_rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => Arc::new(rt),
        Err(e) => {
            tracing::error!("Failed to create Tokio runtime: {:?}", e);
            return;
        }
    };

    let platform = gpui_platform::current_platform(false);
    Application::with_platform(platform)
        .with_assets(AppAssets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let structure = Structure::new();

            let app_theme = AppTheme::new();

            let theme = gpui_component::Theme::global_mut(cx);
            theme.muted_foreground = app_theme.text.muted;
            theme.caret = app_theme.text.normal;
            theme.font_size = structure.font_size;
            theme.primary = app_theme.accent;
            theme.border = app_theme.tile.border;
            theme.font_family = "Noto Sans".into();

            cx.set_global(structure);
            cx.set_global(app_theme.clone());

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Maximized(Bounds::maximized(None, cx))),
                titlebar: Some(TitlebarOptions {
                    title: Some(APP_HUMAN_NAME.into()),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let tokio_rt = Arc::clone(&tokio_rt);
            if let Err(e) = load_keymap_from_json(include_str!("../keybindings/default.json"), cx) {
                tracing::error!("Failed to load keymap: {:?}", e);
            }
            
            cx.observe_keystrokes(|e, _, _| tracing::trace!("{:?}", e.context_stack))
                .detach();
            
            if let Err(e) = cx.open_window(options, |window, cx| {
                let root_view = cx.new(|cx| RootView::new(tokio_rt, window, cx));
                cx.new(|cx| Root::new(root_view, window, cx))
            }) {
                tracing::error!("Failed to open window: {:?}", e);
            }

            cx.activate(true);
        });
}
