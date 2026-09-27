use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use chrono_tz::Tz;
use enumset::EnumSet;
use macros::matrix_settings;
use matrix_sdk::Client;
use ruma::events::AnyGlobalAccountDataEventContent;
use ruma::serde::Raw;
use serde::de::DeserializeOwned;
use serde_json::value::to_raw_value;
use tokio::sync::watch;
use toml_edit::{DocumentMut, Item, Table, Value};

use super::update::{get_field_cloud, get_field_local, set_field_cloud};
use super::{CLOUD_TABLE, NameDecoration, SETTINGS_TABLE};
use serde::{Deserialize, Serialize};

use crate::settings::{
    DEFAULT_SYSTEM_MESSAGES, DataSizeUnit, DateFormat, DayOfWeek, HourFormat, SettingsSection,
    SystemMessageType,
};

#[derive(Debug)]
pub struct MatrixSettingField<T: 'static> {
    val: watch::Sender<T>,
    pub type_name: &'static str,
    pub human_readable: &'static str,
    pub local_name: &'static str,
    pub cloud_name: &'static str,
    pub uses_cloud: Option<watch::Sender<bool>>,
    pub description: &'static str,
    pub section: SettingsSection,
    any_change: watch::Sender<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSearchResult {
    pub type_name: &'static str,
    pub human_readable: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSetting<T: 'static> {
    value: T,
    last_changed: i64,
}

impl<T: Clone + 'static> Clone for MatrixSettingField<T> {
    fn clone(&self) -> Self {
        MatrixSettingField {
            val: self.val.clone(),
            type_name: self.type_name,
            human_readable: self.human_readable,
            local_name: self.local_name,
            cloud_name: self.cloud_name,
            uses_cloud: self.uses_cloud.clone(),
            description: self.description,
            section: self.section,
            any_change: self.any_change.clone(),
        }
    }
}

impl<T> MatrixSettingField<T>
where
    T: Clone + Serialize + DeserializeOwned,
{
    /// Cheap snapshot of the current value.
    pub fn value(&self) -> T {
        self.val.borrow().clone()
    }

    /// Subscribe to reactive updates of the current value.
    pub fn watch(&self) -> watch::Receiver<T> {
        self.val.subscribe()
    }

    fn uses_cloud(&self) -> bool {
        self.uses_cloud
            .as_ref()
            .map(|c| *c.borrow())
            .unwrap_or(false)
    }

    /// Subscribe to reactive updates of whether this field is cloud-synced.
    /// Returns `None` if this field never supports cloud sync.
    pub fn watch_uses_cloud(&self) -> Option<watch::Receiver<bool>> {
        self.uses_cloud.as_ref().map(|c| c.subscribe())
    }

    pub fn to_raw(&self) -> Result<Raw<AnyGlobalAccountDataEventContent>> {
        let now = chrono::Utc::now();

        let setting = CloudSetting {
            value: self.value(),
            last_changed: now.timestamp(),
        };
        to_raw_value(&setting)
            .map(Raw::from_json)
            .map_err(|e| anyhow!(e))
    }

    fn insert_into_toml(&self, table: &mut Table) -> Result<()> {
        let toml_value = self
            .value()
            .serialize(toml_edit::ser::ValueSerializer::new())?;

        let existing = table.get(self.local_name).and_then(|item| item.as_value());
        if existing.map(|v| v.to_string()) == Some(toml_value.to_string()) {
            return Ok(());
        }

        if let Some(item) = table.get_mut(self.local_name) {
            *item = Item::Value(toml_value);
        } else {
            table.insert(self.local_name, Item::Value(toml_value));
        }

        Ok(())
    }

    fn insert_into_toml_with_description(&self, table: &mut Table) -> Result<()> {
        let toml_value = self
            .value()
            .serialize(toml_edit::ser::ValueSerializer::new())?;
        table.insert(self.local_name, Item::Value(toml_value));

        if let Some(mut key) = table.key_mut(self.local_name) {
            key.leaf_decor_mut()
                .set_prefix(format!("# {}\n", self.description));
        }

        Ok(())
    }

    fn insert_into_toml_cloud(&self, table: &mut Table) {
        if self.uses_cloud.is_none() {
            return;
        }

        table.insert(
            self.local_name,
            Item::Value(Value::Boolean(toml_edit::Formatted::new(self.uses_cloud()))),
        );
    }

    fn load_uses_cloud(&self, table: &Table) {
        let Some(cell) = &self.uses_cloud else {
            return;
        };

        if let Some(value) = table.get(self.local_name).and_then(|item| item.as_bool()) {
            cell.send_replace(value);
        }
    }

    pub fn set_uses_cloud(&self, uses_cloud: bool, settings: &Settings) {
        let Some(cell) = &self.uses_cloud else {
            tracing::warn!("Cannot set uses_cloud to true when uses_cloud is not set");
            return;
        };
        cell.send_replace(uses_cloud);

        {
            let mut document = settings
                .document
                .lock()
                .unwrap_or_else(|posion| posion.into_inner());
            let Some(table) = document
                .get_mut(CLOUD_TABLE)
                .and_then(|item| item.as_table_mut())
            else {
                tracing::warn!("Cloud table not found, not updating table");
                return;
            };

            self.insert_into_toml_cloud(table);
        }
        settings.save();
    }

    pub async fn set(&self, val: T, settings: &Settings) {
        self.val.send_replace(val);
        self.any_change.send_replace(());

        {
            let mut document = settings
                .document
                .lock()
                .unwrap_or_else(|posion| posion.into_inner());
            let Some(item) = document.get_mut(SETTINGS_TABLE) else {
                tracing::warn!("Settings table not found, not updating table");
                return;
            };
            let Some(table) = item.as_table_mut() else {
                tracing::warn!("Settings table not found, not updating table");
                return;
            };

            if let Err(e) = self.insert_into_toml(table) {
                tracing::error!("Failed to insert into TOML: {:?}", e);
            }
        }
        settings.save();

        if self.uses_cloud()
            && let Err(e) = set_field_cloud(&settings.client, self).await
        {
            tracing::error!("Failed to save setting {}: {:?}", self.type_name, e);
        };
    }

    async fn refresh(
        &self,
        document: &Mutex<DocumentMut>,
        client: &Client,
        file_last_changed: i64,
        default: T,
    ) {
        let mut missing_from_cloud = false;
        let cloud_setting = if self.uses_cloud() {
            match get_field_cloud::<T>(client.clone(), self.cloud_name).await {
                Ok(Some(setting)) => Some(setting),
                Ok(None) => {
                    missing_from_cloud = true;
                    None
                }
                Err(e) => {
                    tracing::error!("Failed to get field {} from cloud: {:?}", self.type_name, e);
                    missing_from_cloud = true;
                    None
                }
            }
        } else {
            None
        };

        {
            let mut document = document.lock().unwrap_or_else(|posion| posion.into_inner());
            let Some(table) = document
                .get_mut(SETTINGS_TABLE)
                .and_then(|item| item.as_table_mut())
            else {
                tracing::warn!("Settings table not found, not updating table");
                return;
            };

            let local_setting = get_field_local(table, self.local_name, default.clone())
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to get local setting {}: {:?}", self.type_name, e);
                    default
                });

            let newer = match &cloud_setting {
                Some(cs) if cs.last_changed > file_last_changed => cs.value.clone(),
                _ => local_setting,
            };

            self.val.send_replace(newer);
            self.any_change.send_replace(());
            if let Err(e) = self.insert_into_toml(table) {
                tracing::error!("Failed to insert into TOML: {:?}", e);
            }
        }

        if missing_from_cloud && let Err(e) = set_field_cloud(client, self).await {
            tracing::error!(
                "Failed to seed cloud value for setting {}: {:?}",
                self.type_name,
                e
            );
        }
    }
}

#[matrix_settings(namespace = "settings")]
pub struct Settings {
    #[setting(
        name = "Scaling",
        description = "The scaling factor for the application",
        section = SettingsSection::Appearance,
        uses_cloud = None,
        default = 1.0
    )]
    pub scaling: f64,
    #[setting(
        name = "Enable Url Previews per room",
        description = "Whether to show URL previews per room",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = HashMap::new()
    )]
    pub url_previews: HashMap<String, bool>,
    #[setting(
        name = "Show url previews by default",
        description = "Whether to show URL previews by default when not specified per room",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = false
    )]
    pub url_previews_default: bool,
    #[setting(
        name = "Show image border",
        description = "Whether to show the image border",
        section = SettingsSection::Appearance,
        uses_cloud = Some(true),
        default = true
    )]
    pub show_image_border: bool,
    #[setting(
        name = "Automatically download updates",
        description = "Whether to automatically download updates when a new version is available",
        section = SettingsSection::Updates,
        uses_cloud = Some(true),
        default = false
    )]
    pub auto_download_update: bool,
    #[setting(
        name = "Notify when an update is available",
        description = "Whether to notify the user when an update is available",
        section = SettingsSection::Updates,
        uses_cloud = Some(true),
        default = true
    )]
    pub notify_update: bool,
    #[setting(
        name = "Show read markers",
        description = "Whether to show read markers in the chat",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = true
    )]
    pub show_read_markers: bool,
    #[setting(
        name = "Send read markers",
        description = "Whether to send read markers to the server",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = true
    )]
    pub send_read_markers: bool,
    #[setting(
        name = "Show typing indicators",
        description = "Whether to show typing indicators in the chat",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = true
    )]
    pub show_typing_indicators: bool,
    #[setting(
        name = "Send typing indicators",
        description = "Whether to send typing indicators to the server",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = true
    )]
    pub send_typing_indicators: bool,
    #[setting(
        name = "Timezone",
        description = "The timezone to use for the chat",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = chrono_tz::Tz::UTC
    )]
    pub timezone: Tz,
    #[setting(
        name = "Data size unit",
        description = "The unit to use for data size",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = DataSizeUnit::Mibibytes
    )]
    pub data_size_unit: DataSizeUnit,
    #[setting(
        name = "Hour format",
        description = "The hour format to use for timestamps",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = HourFormat::TwentyFourHour
    )]
    pub hour_format: HourFormat,
    #[setting(
        name = "Date format",
        description = "The date format to use for timestamps",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = DateFormat::DayMonthYear
    )]
    pub date_format: DateFormat,
    #[setting(
        name = "First day of week",
        description = "The first day of the week",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = DayOfWeek::Monday
    )]
    pub first_day_of_week: DayOfWeek,
    #[setting(
        name = "Mark pinned messages",
        description = "Whether to mark pinned messages visually in the chat",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = true
    )]
    pub mark_pinned_messages: bool,
    #[setting(
        name = "Which system messages to show",
        description = "Which system messages to show in the chat",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = DEFAULT_SYSTEM_MESSAGES
    )]
    pub system_messages_to_show: EnumSet<SystemMessageType>,
    #[setting(
        name = "Minimize to tray",
        description = "Whether to minimize the window to the system tray",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = true
    )]
    pub minimize_to_tray: bool,
    #[setting(
        name = "Play user theme on click",
        description = "Whether to play the user's theme on click",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = true
    )]
    pub play_user_theme_on_click: bool,
    #[setting(
        name = "Use name colors",
        description = "Whether to use name colors in the chat",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = true
    )]
    pub use_name_color: bool,
    #[setting(
        name = "Use banner colors",
        description = "Whether to use banner colors in the chat",
        section = SettingsSection::General,
        uses_cloud = Some(true),
        default = true
    )]
    pub use_banner_colors: bool,
    #[setting(
        name = "Name decoration",
        description = "How to decorate names in the chat",
        section = SettingsSection::Appearance,
        uses_cloud = Some(true),
        default = NameDecoration::FirstLetter
    )]
    pub name_decoration: NameDecoration,
}
