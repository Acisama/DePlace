use matrix_sdk::Client;

use crate::components::GenericState;

pub struct Discovery {
    homeserver_url: String,
    state: GenericState<Client>,
}

impl Default for Discovery {
    fn default() -> Self {
        Self {
            homeserver_url: "erik-is.gay".to_string(),
            state: GenericState::default(),
        }
    }
}

impl Discovery {
    pub fn new(client: Client) -> Self {
        Self {
            homeserver_url: client.homeserver().to_string(),
            state: GenericState::Success(client),
        }
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
            state: GenericState::default(),
        }
    }
}
