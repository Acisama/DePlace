use matrix_sdk::{
    Client,
    ruma::{OwnedDeviceId, OwnedUserId},
};

pub struct UserDevice {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
}

#[derive(Clone)]
pub struct AppState {
    client: Client,
}

impl AppState {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}
