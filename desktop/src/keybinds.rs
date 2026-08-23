use anyhow::Result;
use std::{collections::HashMap, io::ErrorKind, path::PathBuf};

use gpui::{Action, App};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Deserialize, Serialize, Debug)]
pub struct KeymapSection {
    pub context: Option<String>,
    pub bindings: HashMap<String, JsonValue>,
}

pub type KeymapFile = Vec<KeymapSection>;

fn parse_action(
    action_name: &str,
    action_args: Option<&JsonValue>,
    cx: &App,
) -> Option<Box<dyn Action>> {
    cx.build_action(action_name, action_args.cloned()).ok()
}

use gpui::{KeyBinding, KeyBindingContextPredicate};
use std::rc::Rc;

pub fn create_keybinding(
    keystroke: &str,
    action: Box<dyn gpui::Action>,
    context_str: Option<&str>,
    use_key_equivalents: bool,
    cx: &App,
) -> Result<KeyBinding, anyhow::Error> {
    let context_predicate = if let Some(ctx) = context_str {
        let predicate = KeyBindingContextPredicate::parse(ctx)?;
        Some(Rc::new(predicate))
    } else {
        None
    };

    let keyboard_mapper = cx.keyboard_mapper();

    let binding = KeyBinding::load(
        keystroke,
        action,
        context_predicate,
        use_key_equivalents,
        None,
        keyboard_mapper.as_ref(),
    )?;

    Ok(binding)
}

fn default_keymap() -> Vec<KeymapSection> {
    let mut keybind = KeymapSection {
        context: Some("!Vim && !Input".to_string()),
        bindings: HashMap::from([(
            "space".to_string(),
            serde_json::json!(["chat::FocusInputWithKey", { "key": " " }]),
        )]),
    };

    for char in [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        '`', '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/',
    ] {
        keybind.bindings.insert(
            char.to_string(),
            serde_json::json!(["chat::FocusInputWithKey", { "key": char.to_string() }]),
        );
    }

    for char in [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        '`', '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/',
    ] {
        keybind.bindings.insert(
            format!("shift-{}", char),
            serde_json::json!(["chat::FocusInputWithKey", { "key": char.to_ascii_uppercase().to_string() }]),
        );
    }

    let keybinds = vec![
        KeymapSection {
            context: Some("!Overlay".to_string()),
            bindings: HashMap::from([
                (
                    "ctrl-k".to_string(),
                    serde_json::json!(["overlay::Open", "QuickSelect"]),
                ),
                (
                    "ctrl-,".to_string(),
                    serde_json::json!(["overlay::Open", "Settings"]),
                ),
            ]),
        },
        KeymapSection {
            context: Some("Settings".to_string()),
            bindings: HashMap::from([
                ("ctrl-,".to_string(), serde_json::json!("settings::Close")),
                ("escape".to_string(), serde_json::json!("settings::Close")),
            ]),
        },
        KeymapSection {
            context: Some("QuickSelect".to_string()),
            bindings: HashMap::from([
                (
                    "ctrl-k".to_string(),
                    serde_json::json!("quick_select::Close"),
                ),
                (
                    "enter".to_string(),
                    serde_json::json!("quick_select::Confirm"),
                ),
                (
                    "tab".to_string(),
                    serde_json::json!("quick_select::FocusNext"),
                ),
                (
                    "shift-tab".to_string(),
                    serde_json::json!("quick_select::FocusPrevious"),
                ),
                (
                    "down".to_string(),
                    serde_json::json!("quick_select::FocusNext"),
                ),
                (
                    "up".to_string(),
                    serde_json::json!("quick_select::FocusPrevious"),
                ),
                (
                    "escape".to_string(),
                    serde_json::json!("quick_select::Close"),
                ),
            ]),
        },
        KeymapSection {
            context: Some("Vim && !Input".to_string()),
            bindings: HashMap::from([
                ("j".to_string(), serde_json::json!("chat::FocusNext")),
                ("k".to_string(), serde_json::json!("chat::FocusPrevious")),
                ("i".to_string(), serde_json::json!("chat::FocusInput")),
                ("e".to_string(), serde_json::json!("chat::EditMessage")),
            ]),
        },
        KeymapSection {
            context: Some("EditMessage".to_string()),
            bindings: HashMap::from([
                ("escape".to_string(), serde_json::json!("chat::CancelEdit")),
                ("enter".to_string(), serde_json::json!("chat::SubmitEdit")),
            ]),
        },
        KeymapSection {
            context: Some("(Home > Input) && !(Overlay > Input) && !EditMessage".to_string()),
            bindings: HashMap::from([
                (
                    "escape".to_string(),
                    serde_json::json!("chat::UnfocusInput"),
                ),
                ("enter".to_string(), serde_json::json!("chat::SendMessage")),
            ]),
        },
        KeymapSection {
            context: Some("Home".to_string()),
            bindings: HashMap::from([(
                "ctrl-space".to_string(),
                serde_json::json!("home::ToggleVimMode"),
            )]),
        },
        keybind,
    ];

    keybinds
}

pub fn load_keymap_from_json(path: &PathBuf, cx: &mut App) {
    let keymap_file = if cfg!(debug_assertions) {
        default_keymap()
    } else {
        match std::fs::read_to_string(path).map(|s| serde_json::from_str(&s)) {
            Ok(Ok(map)) => map,
            Ok(Err(e)) => {
                tracing::error!("Failed to deserialize keybinds: {e}");
                default_keymap()
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tracing::debug!("Keybinds file not found, creating default");

                let default = default_keymap();

                match serde_json::to_string_pretty(&default) {
                    Ok(default_str) => {
                        if let Err(e) = std::fs::write(path, default_str) {
                            tracing::error!("Failed to write default keymap: {e}");
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to serialize default keymap: {e}");
                    }
                };

                default
            }
            Err(e) => {
                tracing::error!("Failed to load keybinds: {e}");
                default_keymap()
            }
        }
    };

    let mut key_bindings = Vec::new();

    for section in keymap_file {
        let context_str = section.context.as_deref();

        for (keystroke, action_value) in section.bindings {
            if action_value.is_null() {
                continue;
            }

            let (action_name, args) = match &action_value {
                serde_json::Value::String(s) => (s.as_str(), None),
                serde_json::Value::Array(arr) if !arr.is_empty() => {
                    (arr[0].as_str().unwrap_or_default(), arr.get(1))
                }
                _ => continue,
            };

            if let Ok(action) = cx.build_action(action_name, args.cloned()) {
                match create_keybinding(&keystroke, action, context_str, false, cx) {
                    Ok(binding) => key_bindings.push(binding),
                    Err(err) => eprintln!("Failed to load keybind '{keystroke}': {err}"),
                }
            }
        }
    }
    cx.bind_keys(key_bindings);
}
