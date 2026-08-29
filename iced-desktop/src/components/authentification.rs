use deplace_core::matrix_api::test_server;
use iced::widget::{button, column, text, text_input};
use iced::{Element, Task};
use matrix_sdk::{Client, reqwest::Url};

use crate::components::GenericState;

#[derive(Debug, Clone)]
pub enum DiscoveryMessage {
    UrlChanged(String),
    Checking,
    ClientNotFound,
    ClientFound(Client),
    Continue,
}

impl From<Option<Client>> for DiscoveryMessage {
    fn from(client: Option<Client>) -> Self {
        match client {
            Some(client) => DiscoveryMessage::ClientFound(client),
            None => DiscoveryMessage::ClientNotFound,
        }
    }
}

pub enum DiscoveryAction {
    None,
    Run(Task<Option<Client>>),
    SelectedClient(Client),
}

pub struct Discovery {
    homeserver_url: String,
    state: GenericState<Client>,
    // Aborts the in-flight `test_server` check (if any) when replaced or dropped,
    // so at most one check is ever running at a time.
    current_check: Option<iced::task::Handle>,
}

impl From<Client> for Discovery {
    fn from(client: Client) -> Self {
        Self {
            homeserver_url: client.homeserver().to_string(),
            state: GenericState::Success(client),
            current_check: None,
        }
    }
}

impl Discovery {
    pub fn new() -> (Self, Task<Option<Client>>) {
        let url = url_macro::url!("https://erik-is.gay");

        (
            Self {
                homeserver_url: url.to_string(),
                state: GenericState::Checking,
                current_check: None,
            },
            Task::future(test_server(url)),
        )
    }

    pub fn update(&mut self, message: DiscoveryMessage) -> DiscoveryAction {
        match message {
            DiscoveryMessage::UrlChanged(mut url) => {
                if !url.starts_with("https://") {
                    url = format!("https://{}", url);
                }

                self.homeserver_url = url.clone();
                tracing::trace!("URL changed: {}", url);

                match Url::parse(&url) {
                    Ok(url) => {
                        self.state = GenericState::Checking;

                        let (task, handle) = Task::future(test_server(url)).abortable();
                        self.current_check = Some(handle.abort_on_drop());

                        DiscoveryAction::Run(task)
                    }
                    Err(_) => {
                        self.current_check = None;
                        self.state = GenericState::Error("Invalid URL".to_string());
                        DiscoveryAction::None
                    }
                }
            }
            DiscoveryMessage::Checking => {
                self.state = GenericState::Checking;
                DiscoveryAction::None
            }
            DiscoveryMessage::ClientNotFound => {
                self.state = GenericState::Error("Invalid homeserver URL".to_string());
                DiscoveryAction::None
            }
            DiscoveryMessage::ClientFound(client) => {
                self.state = GenericState::Success(client);
                DiscoveryAction::None
            }
            DiscoveryMessage::Continue => match &self.state {
                GenericState::Success(client) => DiscoveryAction::SelectedClient(client.clone()),
                _ => DiscoveryAction::None,
            },
        }
    }

    pub fn view(&self) -> Element<'_, DiscoveryMessage> {
        let input = text_input("https://matrix.example.org", &self.homeserver_url)
            .on_input(DiscoveryMessage::UrlChanged)
            .padding(10);

        let status = match &self.state {
            GenericState::Checking => text("Checking..."),
            GenericState::Error(error) => text(error.clone()),
            GenericState::Success(_) => text("Homeserver is valid"),
        };

        // `on_press_maybe` takes `Option<Message>`; `None` renders the button disabled,
        // so this is only clickable once a homeserver has actually been found.
        let continue_button = button("Continue").on_press_maybe(
            matches!(self.state, GenericState::Success(_)).then_some(DiscoveryMessage::Continue),
        );

        column![input, status, continue_button]
            .spacing(10)
            .padding(20)
            .into()
    }
}

pub struct Login {
    client: matrix_sdk::Client,
    username: String,
    password: String,
    /// Holds the authenticated client
    state: GenericState<Client>,
}

impl Login {
    pub fn new(client: matrix_sdk::Client) -> Self {
        Self {
            client,
            username: "".to_string(),
            password: "".to_string(),
            state: GenericState::Checking,
        }
    }
}
