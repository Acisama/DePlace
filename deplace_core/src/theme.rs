use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::colors::DePlaceColor;

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct Colors {
    pub red: DePlaceColor,
    pub green: DePlaceColor,
    pub yellow: DePlaceColor,
    pub success: DePlaceColor,
    pub warning: DePlaceColor,
    pub error: DePlaceColor,
    pub idle: DePlaceColor,
    pub online: DePlaceColor,
    pub offline: DePlaceColor,
    pub busy: DePlaceColor,
    pub muted: DePlaceColor,
    pub unknown: DePlaceColor,
}

impl Colors {
    fn new(
        red: DePlaceColor,
        green: DePlaceColor,
        yellow: DePlaceColor,
        muted: DePlaceColor,
        unknown: DePlaceColor,
    ) -> Self {
        Self {
            red,
            green,
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
    pub muted: DePlaceColor,
    pub dim: DePlaceColor,
    pub normal: DePlaceColor,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct InputTheme {
    pub background: DePlaceColor,
    pub focus_background: DePlaceColor,
    pub focused_border: DePlaceColor,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct Theme {
    pub accent: DePlaceColor,
    pub blur: f32,
    pub text: Text,
    pub border: DePlaceColor,
    pub background: DePlaceColor,
    pub solid_hover_bg: DePlaceColor,
    pub solid_bg: DePlaceColor,

    pub input: InputTheme,
    pub colors: Colors,
    pub pill_color: DePlaceColor,
}

impl Default for Theme {
    fn default() -> Self {
        let accent = DePlaceColor::from_hsla(0.53 * 360.0, 0.52, 0.52, 1.0);

        let muted_color = DePlaceColor::from_hsla(0.66 * 360.0, 0.15, 0.3, 1.0);

        let red = DePlaceColor::from_hsla(0.9462 * 360.0, 0.5569, 0.6725, 1.0);
        let green = DePlaceColor::from_hsla(0.3682 * 360.0, 0.5446, 0.6039, 1.0);
        let yellow = DePlaceColor::from_hsla(0.155 * 360.0, 0.786, 0.743, 1.0);

        let solid_bg = DePlaceColor::from_hsla(0.6667 * 360.0, 0.5000, 0.0314, 1.0);
        let solid_hover_bg = DePlaceColor::from_hsla(0.6667 * 360.0, 0.2105, 0.1490, 1.0);

        Self {
            blur: 40.0,
            solid_bg,
            solid_hover_bg,
            pill_color: DePlaceColor::from_rgba(1.0, 1.0, 1.0, 1.0),
            border: DePlaceColor::from_hsla(0.0, 0.0, 0.2, 1.0),
            background: DePlaceColor::from_hsla(0.6667 * 360.0, 0.5000, 0.0314, 0.5),
            text: Text {
                muted: muted_color,
                dim: DePlaceColor::from_hsla(0.66 * 360.0, 0.15, 0.4, 1.0),
                normal: DePlaceColor::from_hsla(0.66 * 360.0, 0.15, 0.7, 1.0),
            },
            input: InputTheme {
                background: DePlaceColor::from_hsla(0.0, 0.0, 0.0, 0.2),
                focus_background: DePlaceColor::from_hsla(0.0, 0.0, 0.0, 0.4),
                focused_border: accent.darken(0.4),
            },
            colors: Colors::new(
                red,
                green,
                yellow,
                muted_color,
                DePlaceColor::from_hsla(0.0, 1.0, 0.7, 1.0),
            ),
            accent,
        }
    }
}

impl Theme {
    pub fn new(file_path: PathBuf) -> Self {
        if cfg!(debug_assertions) {
            return Self::default();
        }

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
