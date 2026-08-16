use chrono_tz::Tz;
use imbl::HashMap;
use macros::matrix_settings;

use super::update::{get_field_cloud, get_field_local, set_field_cloud};
use super::{CLOUD_TABLE, SETTINGS_TABLE};
use serde::{Deserialize, Serialize};

use crate::settings::{
    DataSizeUnit, DateFormat, DayOfWeek, HourFormat, SettingsSection, SystemMessageType,
    default_system_messages_to_show,
};

#[matrix_settings]
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
        default = default_system_messages_to_show()
    )]
    pub system_messages_to_show: HashMap<SystemMessageType, bool>,
    #[setting(
        name = "Minimize to tray",
        description = "Whether to minimize the window to the system tray",
        section = SettingsSection::Chats,
        uses_cloud = Some(true),
        default = false
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
}
