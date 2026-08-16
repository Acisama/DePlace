use macros::matrix_settings;
use serde::{Deserialize, Serialize};
use update::{get_field_cloud, get_field_local, set_field_cloud};

mod update;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsSection {
    Profile,
    General,
    Appearance,
    Audio,
    Chats,
    Updates,
    Divider,
}

impl SettingsSection {
    pub fn id(&self) -> &'static str {
        match self {
            SettingsSection::Profile => "profile",
            SettingsSection::General => "general",
            SettingsSection::Appearance => "appearance",
            SettingsSection::Audio => "audio",
            SettingsSection::Chats => "chats",
            SettingsSection::Updates => "updates",
            SettingsSection::Divider => "divider",
        }
    }
}

#[derive(Clone, Default, PartialEq, Deserialize, Serialize)]
pub enum DataSizeUnit {
    #[default]
    Bytes,
    Bits,
    Mibibytes,
}

#[matrix_settings]
pub struct Settings {
    #[setting(name = "Data size unit", description = "The unit of data size to use", section = SettingsSection::General, default = DataSizeUnit::Bytes, uses_cloud = Some(true))]
    pub data_size_unit: DataSizeUnit,
}

const SETTINGS_TABLE: &str = "settings";
const CLOUD_TABLE: &str = "cloud";
pub const SETTINGS_FILE_NAME: &str = "settings.toml";
