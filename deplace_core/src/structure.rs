use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct ChatSidebarWidth {
    pub member: f32,
    pub search: f32,
    pub pinned: f32,
    pub member_list: f32,
}

impl Default for ChatSidebarWidth {
    fn default() -> Self {
        ChatSidebarWidth {
            member: 320.0,
            search: 480.0,
            pinned: 480.0,
            member_list: 240.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct ServerColumn {
    pub icon_size: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Sidebar {
    pub width: f32,
    pub dm_icon_height: f32,
    pub channel_icon_height: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Header {
    pub height: f32,
    pub icon_size: f32,
    pub button_size: f32,
}

impl Header {
    pub fn button_padding(&self) -> f32 {
        (self.height - self.button_size) / 2.0
    }

    pub fn inner_icon_padding(&self) -> f32 {
        (self.button_size - self.icon_size) / 2.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct ChatSidebar {
    pub large_icon_size: f32,
    pub banner_height: f32,
    pub width: ChatSidebarWidth,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Chat {
    pub icon_size: f32,
    pub text_size: f32,
    pub small_icon_size: f32,
    pub small_text_size: f32,
    pub max_media_height: f32,
    pub max_media_width: f32,
    pub input_height: f32,
    pub attachment_preview_dimensions: (f32, f32),
    pub sidebar: ChatSidebar,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub section_column_width: f32,
    pub full_width: f32,
    pub full_height: f32,
    pub checkbox_width: f32,
    pub checkbox_height: f32,
    pub dropdown_width: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Authentification {
    pub width: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Structure {
    pub server_column: ServerColumn,
    pub divider_width: f32,
    pub border_thickness: f32,
    pub sidebar: Sidebar,
    pub header: Header,
    pub chat: Chat,
    pub outer_border_radius: f32,
    pub inner_border_radius: f32,
    pub smaller_border_radius: f32,
    pub font_size: f32,
    pub small_font_size: f32,
    pub large_font_size: f32,
    pub gap: f32,
    pub small_gap: f32,
    pub icon_gap: f32,
    pub settings: Settings,
    pub authentification: Authentification,
}

impl Default for Structure {
    fn default() -> Self {
        let outer_border_radius = 18.0;
        let gap = 12.0;
        let small_gap = 8.0;
        let inner_border_radius = outer_border_radius - small_gap;
        let smaller_border_radius = inner_border_radius - small_gap;

        Self {
            gap,
            small_gap,
            icon_gap: 0.175,
            inner_border_radius,
            outer_border_radius,
            smaller_border_radius,
            font_size: 15.0,
            small_font_size: 11.0,
            large_font_size: 19.0,
            divider_width: 2.0,
            border_thickness: 1.0,

            chat: Chat {
                icon_size: 40.0,
                small_icon_size: 18.0,
                text_size: 16.0,
                small_text_size: 12.0,
                max_media_height: 500.0,
                max_media_width: 500.0,
                input_height: 50.0,
                attachment_preview_dimensions: (140.0, 100.0),
                sidebar: ChatSidebar {
                    large_icon_size: 80.0,
                    banner_height: 250.0,
                    width: ChatSidebarWidth {
                        member: 320.0,
                        search: 480.0,
                        pinned: 480.0,
                        member_list: 240.0,
                    },
                },
            },
            header: Header {
                height: 50.0,
                icon_size: 20.0,
                button_size: 25.0,
            },
            sidebar: Sidebar {
                width: 300.0,
                dm_icon_height: 30.0,
                channel_icon_height: 20.0,
            },
            server_column: ServerColumn { icon_size: 40.0 },
            settings: Settings {
                section_column_width: 300.0,
                full_width: 1200.0,
                full_height: 900.0,
                checkbox_width: 40.0,
                checkbox_height: 20.0,
                dropdown_width: 200.0,
            },
            authentification: Authentification { width: 360.0 },
        }
    }
}

impl Structure {
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

    pub fn semi_border_radius(&self) -> f32 {
        (self.inner_border_radius + self.smaller_border_radius) / 2.0
    }

    pub fn chat_col_width(&self) -> f32 {
        self.chat.icon_size + 3.0 * self.small_gap
    }

    pub fn server_column_width(&self) -> f32 {
        self.server_column.icon_size + 3.0 * self.small_gap
    }
}
