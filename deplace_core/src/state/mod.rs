use matrix_sdk::{
    Client,
    ruma::{OwnedDeviceId, OwnedUserId},
};

#[derive(Clone)]
pub struct UserDevice {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
}

#[derive(Clone)]
pub struct AppState {
    client: Client,
    user_device: UserDevice,
}

impl AppState {
    pub fn new(client: Client, user_device: UserDevice) -> Self {
        Self {
            client,
            user_device,
        }
    }
}
