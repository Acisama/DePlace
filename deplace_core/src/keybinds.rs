use keyboard_types::{Key, Modifiers, NamedKey};
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct Keybinds {
    pub settings: Shortcut,
    pub quickselect: Shortcut,
    pub overview: Shortcut,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            settings: Shortcut {
                modifiers: Modifiers::CONTROL,
                key: Key::Character(",".to_string()),
            },
            quickselect: Shortcut {
                modifiers: Modifiers::CONTROL,
                key: Key::Character("k".to_string()),
            },
            overview: Shortcut {
                modifiers: Modifiers::CONTROL,
                key: Key::Character("o".to_string()),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers.ctrl() {
            parts.push("Ctrl");
        }
        if self.modifiers.shift() {
            parts.push("Shift");
        }
        if self.modifiers.alt() {
            parts.push("Alt");
        }
        if self.modifiers.meta() {
            parts.push("Super");
        }

        let key_str = self.key.to_string();
        parts.push(key_str.as_str());
        write!(f, "{}", parts.join("+"))
    }
}

#[cfg(feature = "iced_desktop")]
use iced::keyboard::{Event, Key as IcedKey, key::Named};

#[cfg(feature = "iced_desktop")]
impl Shortcut {
    pub fn matches(&self, event: &Event) -> bool {
        if let Event::KeyPressed { key, modifiers, .. } = event {
            self.matches_key(key, modifiers)
        } else {
            false
        }
    }

    /// Same check as [`Shortcut::matches`], but usable from contexts that
    /// only have the raw key/modifiers, not a full [`Event`] -- e.g.
    /// `text_editor`'s `key_binding` hook.
    pub fn matches_key(&self, key: &IcedKey, modifiers: &iced::keyboard::Modifiers) -> bool {
        if modifiers.alt() != self.modifiers.alt() {
            return false;
        }
        if modifiers.control() != self.modifiers.ctrl() {
            return false;
        }
        if modifiers.shift() != self.modifiers.shift() {
            return false;
        }
        if modifiers.logo() != self.modifiers.meta() {
            return false;
        }

        match key {
            IcedKey::Unidentified => false,
            IcedKey::Character(c1) => {
                if let Key::Character(c2) = &self.key {
                    c1 == c2
                } else {
                    false
                }
            }
            IcedKey::Named(named1) => {
                if let Key::Named(named2) = &self.key {
                    match (named1, named2) {
                        (Named::Backspace, NamedKey::Backspace) => true,
                        (Named::Enter, NamedKey::Enter) => true,
                        (Named::Space, _) if self.key == Key::Character(' '.into()) => true,
                        // TODO: add other named key matches here
                        _ => false,
                    }
                } else {
                    false
                }
            }
        }
    }
}

impl Keybinds {
    pub fn new(keybinds_file: PathBuf) -> Self {
        if !keybinds_file.exists() {
            let keybinds = Self::default();

            match toml_edit::ser::to_string(&keybinds) {
                Ok(contents) => {
                    if let Err(e) = std::fs::write(&keybinds_file, contents) {
                        tracing::error!("Failed to write keybinds: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to serialize keybinds: {}", e);
                }
            }
            return keybinds;
        }

        let contents = std::fs::read_to_string(keybinds_file).unwrap_or_default();
        toml_edit::de::from_str(&contents).unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub enum KeybindAction {
    ToggleQuickselect,
    ToggleSettings,
    ToggleOverview,
}
