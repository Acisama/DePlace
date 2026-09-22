use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::colors::Color;

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct Colors {
    pub red: Color,
    pub _green: Color,
    pub yellow: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub idle: Color,
    pub online: Color,
    pub offline: Color,
    pub busy: Color,
    pub muted: Color,
    pub unknown: Color,
}

impl Colors {
    fn new(red: Color, green: Color, yellow: Color, muted: Color, unknown: Color) -> Self {
        Self {
            red,
            _green: green,
            yellow,
            success: green,
            warning: yellow,
            error: red,
            idle: yellow,
            online: green,
            offline: muted,
            busy: red,
            muted,
            unknown,
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct Text {
    pub muted: Color,
    pub dim: Color,
    pub normal: Color,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct InputTheme {
    pub background: Color,
    pub focus_background: Color,
    pub focused_border: Color,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct Theme {
    pub accent: Color,
    pub blur: f32,
    pub text: Text,
    pub border: Color,
    pub background: Color,
    pub solid_hover_bg: Color,
    pub solid_bg: Color,

    pub input: InputTheme,
    pub colors: Colors,
    pub pill_color: Color,
}

impl Default for Theme {
    fn default() -> Self {
        let accent = Color::from_hsla(0.53, 0.52, 0.52, 1.0);

        let muted_color = Color::from_hsla(0.66, 0.15, 0.3, 1.0);

        let red = Color::from_hsla(0.9462, 0.5569, 0.6725, 1.0);
        let green = Color::from_hsla(0.3682, 0.5446, 0.6039, 1.0);
        let yellow = Color::from_hsla(0.155, 0.786, 0.743, 1.0);

        let solid_bg = Color::from_hsla(0.6667, 0.5000, 0.0314, 1.0);
        let solid_hover_bg = Color::from_hsla(0.6667, 0.2105, 0.1490, 1.0);

        Self {
            blur: 20.0,
            solid_bg,
            solid_hover_bg,
            pill_color: Color::from_rgba(1.0, 1.0, 1.0, 1.0),
            border: Color::from_hsla(0.0, 0.0, 0.2, 1.0),
            background: Color::from_hsla(0.6667, 0.5000, 0.0314, 1.0),
            text: Text {
                muted: muted_color,
                dim: Color::from_hsla(0.66, 0.15, 0.4, 1.0),
                normal: Color::from_hsla(0.66, 0.15, 0.7, 1.0),
            },
            input: InputTheme {
                background: Color::from_hsla(0.0, 0.0, 0.0, 0.2),
                focus_background: Color::from_hsla(0.0, 0.0, 0.0, 0.4),
                focused_border: accent.darken(0.4),
            },
            colors: Colors::new(
                red,
                green,
                yellow,
                muted_color,
                Color::from_hsla(0.0, 1.0, 0.7, 1.0),
            ),
            accent,
        }
    }
}

impl Theme {
    pub fn new(file_path: PathBuf) -> Self {
        if !file_path.exists() {
            let default = Self::default();

            match toml_edit::ser::to_string(&default) {
                Ok(content) => {
                    if let Err(e) = std::fs::write(&file_path, content) {
                        tracing::error!("Failed to write default structure: {}", e);
                    }
                }
                Err(e) => tracing::error!("Failed to serialize default structure: {}", e),
            };

            default
        } else {
            let content = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to read structure: {}", e);
                    return Self::default();
                }
            };
            match toml_edit::de::from_str(&content) {
                Ok(structure) => structure,
                Err(e) => {
                    tracing::error!("Failed to deserialize structure: {}", e);
                    Self::default()
                }
            }
        }
    }
}
