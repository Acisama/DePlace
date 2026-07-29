use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum CoreCommand {
    OpenUi,
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CoreEvent {
    Pong,
    StateUpdated(String),
}
