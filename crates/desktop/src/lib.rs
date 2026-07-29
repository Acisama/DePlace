use gpui::{App, Application, Window, WindowOptions, div, prelude::*, rgb};

struct HelloWorld;

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .bg(rgb(0x18181b))
            .size_full()
            .justify_center()
            .items_center()
            .text_color(rgb(0xf4f4f5))
            .child("Hello World")
    }
}

pub fn run_ui() {
    Application::new().run(|cx: &mut App| {
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| HelloWorld))
            .unwrap();
    });
}

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::tokio::Listener as TokioListener;
use interprocess::local_socket::traits::tokio::Stream;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName};
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, Write};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;

const SOCKET_NAME: &str = "deplace.sock";

#[derive(Serialize, Deserialize)]
pub enum CoreCommand {
    OpenUi,
    Ping,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args: Vec<String> = env::args().collect();

    // only get's called internally
    if args.len() > 1 && args[1] == "--ui-child" {
        println!("Spawning UI");
        run_ui();

        // Exit instead of spawning daemon
        return Ok(());
    }

    let namespaced_socket_name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;

    // Try finding out if a daemon is already running and if yes, signal it to open the ui
    if let Ok(mut stream) = LocalSocketStream::connect(namespaced_socket_name.clone()) {
        println!("Daemon is running. Signaling to open ui");

        let msg = serde_json::to_vec(&CoreCommand::OpenUi)?;
        stream.write_all(&msg)?;
        stream.flush()?;

        // Exit instead of spawning new daemon
        return Ok(());
    }

    // Since no daemon is running, we aquire the socket and run as the daemon
    println!("No daemon running yet");

    let opts = ListenerOptions::new().name(namespaced_socket_name);
    let listener = match opts.create_tokio() {
        Ok(l) => l,
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            println!("Socket in use, cleaning up");
            let _ = std::fs::remove_file(SOCKET_NAME);
            todo!("Spawn the socket after cleaning up")
        }
        Err(e) => return Err(e.into()),
    };

    // Thread-safe handle to ui child process
    let ui_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    // Spawn IPC listener task
    let ui_child_ipc = Arc::clone(&ui_child);
    tokio::spawn(async move {
        listen_for_ipc_commands(listener, ui_child_ipc).await;
    });

    // Automatically spawn UI on initial daemon launch
    if let Err(e) = spawn_or_focus_ui(&ui_child) {
        eprintln!("Failed to spawn ui: {}", e);
    }

    // keep daemon alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
    }
}

async fn listen_for_ipc_commands(
    listener: TokioListener,
    ui_child_handle: Arc<Mutex<Option<Child>>>,
) {
    loop {
        let stream =
            match interprocess::local_socket::traits::tokio::Listener::accept(&listener).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Couldn't accept IPC connection: {}", e);
                    continue;
                }
            };

        let ui_child = ui_child_handle.clone();
        tokio::spawn(async move {
            let (mut reader, _) = stream.split();
            let mut buf = vec![0u8; 1024];

            if let Ok(bytes_read) = reader.read(&mut buf).await {
                if let Ok(cmd) = serde_json::from_slice::<CoreCommand>(&buf[..bytes_read]) {
                    match cmd {
                        CoreCommand::OpenUi => {
                            println!("Received command to open ui");
                            if let Err(e) = spawn_or_focus_ui(&ui_child) {
                                eprintln!("Failed to spawn or focus ui: {}", e);
                            }
                        }
                        CoreCommand::Ping => {
                            // Pong or something i dunno
                        }
                    }
                }
            }
        });
    }
}

fn spawn_or_focus_ui(ui_child_handle: &Arc<Mutex<Option<Child>>>) -> io::Result<()> {
    let mut guard = ui_child_handle.lock().unwrap();

    // See if child is already running
    let is_running = match guard.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(None) => true,
            _ => false,
        },
        None => false,
    };

    if is_running {
        println!("Focusing window");
        todo!("Actually focus window")
    } else {
        println!("Spawning new ui child");
        let current_exe = env::current_exe()?;
        let child = Command::new(current_exe).arg("--ui-child").spawn()?;
        *guard = Some(child);
    }

    Ok(())
}
