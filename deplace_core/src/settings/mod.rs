use imbl::HashMap;
use macros::{EnumConstVec, EnumVariants};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

mod definition;
mod update;

pub use definition::Settings;

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

pub trait EnumVariants: Sized + Serialize + DeserializeOwned {
    fn all_variants() -> impl Iterator<Item = (Self, &'static str)>;
}

impl EnumVariants for chrono_tz::Tz {
    fn all_variants() -> impl Iterator<Item = (Self, &'static str)> {
        chrono_tz::TZ_VARIANTS.iter().map(|tz| (*tz, tz.name()))
    }
}

#[derive(Clone, PartialEq, Deserialize, Serialize, EnumVariants)]
pub enum HourFormat {
    #[serde(rename = "12-hour")]
    TwelveHour,
    #[serde(rename = "24-hour")]
    TwentyFourHour,
}

#[derive(Clone, PartialEq, Deserialize, Serialize, EnumVariants)]
pub enum DateFormat {
    #[serde(rename = "DD/MM/YYYY")]
    DayMonthYear,
    #[serde(rename = "MM/DD/YYYY")]
    MonthDayYear,
    #[serde(rename = "YYYY/MM/DD")]
    YearMonthDay,
}

#[derive(Clone, PartialEq, Deserialize, Serialize, EnumVariants)]
pub enum DayOfWeek {
    #[serde(rename = "Monday")]
    Monday,
    #[serde(rename = "Tuesday")]
    Tuesday,
    #[serde(rename = "Wednesday")]
    Wednesday,
    #[serde(rename = "Thursday")]
    Thursday,
    #[serde(rename = "Friday")]
    Friday,
    #[serde(rename = "Saturday")]
    Saturday,
    #[serde(rename = "Sunday")]
    Sunday,
}

#[derive(Clone, Default, PartialEq, Deserialize, Serialize, EnumVariants)]
pub enum DataSizeUnit {
    #[default]
    Bytes,
    Bits,
    Mibibytes,
}

#[derive(Clone, Copy, PartialEq, Deserialize, Serialize, EnumVariants, EnumConstVec, Hash, Eq)]
pub enum SystemMessageType {
    CallInvite,
    MembershipChange,
    ProfileChange,
    RtcNotification,
    PolicyRuleRoom,
    PolicyRuleServer,
    PolicyRuleUser,
    RoomAvatar,
    RoomCanonicalAlias,
    RoomCreate,
    RoomEncryption,
    RoomGuestAccess,
    RoomHistoryVisibility,
    RoomJoinRules,
    RoomName,
    RoomPinnedEvents,
    RoomPowerLevels,
    RoomServerAcl,
    RoomThirdPartyInvite,
    RoomTombstone,
    RoomTopic,
    SpaceChild,
    SpaceParent,
    Unknown,
    Invisible,
}

const DEFAULT_SYSTEM_MESSAGES: &[SystemMessageType] = &[
    SystemMessageType::MembershipChange,
    SystemMessageType::RoomCreate,
    SystemMessageType::RoomEncryption,
    SystemMessageType::RoomPinnedEvents,
    SystemMessageType::SpaceChild,
    SystemMessageType::SpaceParent,
    SystemMessageType::Unknown,
];

pub const SYSTEM_MESSAGE_MODES: &[(&str, &[SystemMessageType])] = &[
    ("None", &[]),
    ("Default", DEFAULT_SYSTEM_MESSAGES),
    ("Full", SystemMessageType::const_vec()),
];

pub fn default_system_messages_to_show() -> HashMap<SystemMessageType, bool> {
    let mut map = SystemMessageType::all_variants()
        .map(|(variant, _)| (variant, false))
        .collect::<HashMap<_, _>>();
    for message in DEFAULT_SYSTEM_MESSAGES {
        map.insert(*message, true);
    }
    map
}

const SETTINGS_TABLE: &str = "settings";
const CLOUD_TABLE: &str = "cloud";
pub const SETTINGS_FILE_NAME: &str = "settings.toml";
