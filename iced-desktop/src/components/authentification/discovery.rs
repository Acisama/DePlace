use std::hash::Hash;

use deplace_core::matrix_api::test_server;
use iced::widget::{Id, button::Status};
// use iced::{
//     Border, Element,
//     Length::Fill,
//     Task,
//     widget::{self as w, Id, Space, button::Status, column, container, text},
// };
// use matrix_sdk::Client;
use crate::common::*;
use url::Url;

use crate::components::{GenericState, floating_tile, text_input, weighted_text};

#[derive(Debug, Clone)]
pub enum DiscoveryMessage {
    UrlChanged(String),
    Checking,
    ClientNotFound,
    ClientFound(Client),
    Continue,
    ClientSelected(Client),
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
    Run(Task<Option<Client>>),
    ClientSelected(Client),
}

pub struct Discovery {
    homeserver_url: String,
    state: GenericState<Client>,
    // Aborts the in-flight `test_server` check (if any) when replaced or dropped,
    // so at most one check is ever running at a time.
    current_check: Option<iced::task::Handle>,
}

impl Hash for Discovery {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.homeserver_url.hash(state);
        self.state.hash(state);
    }
}

const SERVER_INPUT_ID: Id = Id::new("discovery-input");

impl Discovery {
    pub fn from_client(client: Client) -> (Self, Task<DiscoveryMessage>) {
        let dis = Self {
            homeserver_url: client.homeserver().to_string(),
            state: GenericState::Success(client),
            current_check: None,
        };

        (dis, w::operation::focus(SERVER_INPUT_ID))
    }

    pub fn new() -> (Self, Task<Option<Client>>) {
        let url = url_macro::url!("https://erik-is.gay");

        (
            Self {
                homeserver_url: url.to_string(),
                state: GenericState::Checking,
                current_check: None,
            },
            w::operation::focus(SERVER_INPUT_ID).chain(Task::future(test_server(url))),
        )
    }
}

impl IcedWidget<DiscoveryMessage, DiscoveryAction> for Discovery {
    fn update(&mut self, message: DiscoveryMessage) -> Option<DiscoveryAction> {
        match message {
            DiscoveryMessage::ClientSelected(client) => {
                Some(DiscoveryAction::ClientSelected(client))
            }
            DiscoveryMessage::UrlChanged(mut url) => {
                self.homeserver_url = url.clone();

                if url.is_empty() {
                    self.state = GenericState::Error("Enter a valid URL".to_string());
                    return None;
                }

                if !url.starts_with("https://") {
                    url = format!("https://{}", url);
                }

                tracing::trace!("URL changed: {}", url);

                match Url::parse(&url) {
                    Ok(url) => {
                        self.state = GenericState::Checking;

                        let (task, handle) = Task::future(test_server(url)).abortable();
                        self.current_check = Some(handle.abort_on_drop());

                        Some(DiscoveryAction::Run(task))
                    }
                    Err(_) => {
                        self.current_check = None;
                        self.state = GenericState::Error("Invalid URL".to_string());
                        None
                    }
                }
            }
            DiscoveryMessage::Checking => {
                self.state = GenericState::Checking;
                None
            }
            DiscoveryMessage::ClientNotFound => {
                self.state = GenericState::Error("Invalid homeserver URL".to_string());
                None
            }
            DiscoveryMessage::ClientFound(client) => {
                self.state = GenericState::Success(client);
                None
            }
            DiscoveryMessage::Continue => match &self.state {
                GenericState::Success(client) => {
                    Some(DiscoveryAction::ClientSelected(client.clone()))
                }
                _ => None,
            },
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, DiscoveryMessage> {
        let input: iced::widget::TextInput<'static, DiscoveryMessage> = text_input(
            "https://matrix.example.org",
            &self.homeserver_url,
            theme,
            structure,
        )
        .width(Fill)
        .id(SERVER_INPUT_ID)
        .on_input(DiscoveryMessage::UrlChanged)
        .on_submit_maybe(self.state.success().map(DiscoveryMessage::ClientSelected));

        let status: iced::widget::text::Rich<'static, (), DiscoveryMessage> =
            self.state
                .text("Homeserver is valid", "", &theme.colors, &structure);

        let continue_button = w::button(w::text("Continue").width(Fill).center())
            .width(Fill)
            .style(move |_, status| w::button::Style {
                background: Some(match status {
                    Status::Disabled => theme.colors.muted.into(),
                    Status::Active => theme.accent.into(),
                    _ => theme.accent.scale_alpha(0.5).into(),
                }),
                text_color: theme.background.into(),
                border: Border {
                    radius: structure.semi_border_radius().into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press_maybe(
                self.state
                    .success()
                    .is_some()
                    .then_some(DiscoveryMessage::Continue),
            );

        let tile = floating_tile(
            theme,
            structure,
            w::column![
                weighted_text("Discovery", iced::font::Weight::ExtraBold)
                    .size(structure.large_font_size)
                    .color(theme.accent)
                    .width(Fill)
                    .center(),
                w::column![
                    w::text("Homeserver")
                        .size(structure.font_size)
                        .color(theme.text.dim),
                    Space::new().height(structure.small_gap),
                    input,
                ],
                status,
                continue_button
            ]
            .width(structure.authentification.width)
            .spacing(structure.gap)
            .padding(structure.gap),
        );

        w::container(tile).center(Fill).into()
    }
}
