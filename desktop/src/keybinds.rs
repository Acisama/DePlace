use std::collections::HashMap;

use gpui::{Action, App};
use serde::Deserialize;
use serde_json::Value as JsonValue;

#[derive(Deserialize, Debug)]
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

use anyhow::{Context, Result};

pub fn load_keymap_from_json(json_str: &str, cx: &mut App) -> Result<()> {
    let keymap_file: KeymapFile =
        serde_json::from_str(json_str).context("Failed to parse keymap JSON")?;

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

    Ok(())
}
