use enumset::EnumSet;
use enumset::EnumSetType;
use enumset::enum_set;
use macros::{EnumConstVec, EnumVariants};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

mod definition;
mod update;

pub use definition::MatrixSettingField;
pub use definition::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsSection {
    #[default]
    Profile,
    General,
    Appearance,
    Audio,
    Chats,
    Updates,
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
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            SettingsSection::Profile => "Profile",
            SettingsSection::General => "General",
            SettingsSection::Appearance => "Appearance",
            SettingsSection::Audio => "Audio",
            SettingsSection::Chats => "Chats",
            SettingsSection::Updates => "Updates",
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, EnumVariants)]
pub enum HourFormat {
    #[serde(rename = "12-hour")]
    TwelveHour,
    #[serde(rename = "24-hour")]
    TwentyFourHour,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, EnumVariants)]
pub enum DateFormat {
    #[serde(rename = "DD/MM/YYYY")]
    DayMonthYear,
    #[serde(rename = "MM/DD/YYYY")]
    MonthDayYear,
    #[serde(rename = "YYYY/MM/DD")]
    YearMonthDay,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize, EnumVariants)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize, EnumVariants)]
pub enum DataSizeUnit {
    #[default]
    Bytes,
    Bits,
    Mibibytes,
}

#[derive(Debug, Deserialize, Serialize, EnumVariants, EnumConstVec, Hash, EnumSetType)]
#[enumset(serialize_repr = "list")]
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

const DEFAULT_SYSTEM_MESSAGES: EnumSet<SystemMessageType> = enum_set!(
    SystemMessageType::MembershipChange
        | SystemMessageType::ProfileChange
        | SystemMessageType::RoomCreate
        | SystemMessageType::RoomEncryption
        | SystemMessageType::RoomPinnedEvents
        | SystemMessageType::SpaceChild
        | SystemMessageType::SpaceParent
        | SystemMessageType::Unknown
);

pub const SYSTEM_MESSAGE_MODES: &[(&str, &str, EnumSet<SystemMessageType>)] = &[
    ("None", "Show no system messages", EnumSet::empty()),
    (
        "Discord",
        "Only show discord-like system messages",
        enum_set!(SystemMessageType::MembershipChange | SystemMessageType::RoomPinnedEvents),
    ),
    (
        "Default",
        "Default system messages",
        DEFAULT_SYSTEM_MESSAGES,
    ),
    ("Full", "Show all system messages", EnumSet::all()),
];

const SETTINGS_TABLE: &str = "settings";
const CLOUD_TABLE: &str = "cloud";
pub const SETTINGS_FILE_NAME: &str = "settings.toml";
