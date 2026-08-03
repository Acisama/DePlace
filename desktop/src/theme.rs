use gpui::{App, Global, Hsla, Pixels, hsla, px, white};
use gpui_component::Colorize;

#[derive(Clone)]
pub struct TileTheme {
    pub background: Hsla,
    pub border: Hsla,
}

#[derive(Clone)]
pub struct Text {
    pub muted: Hsla,
    pub dim: Hsla,
    pub normal: Hsla,
}

#[derive(Clone)]
pub struct InputTheme {
    pub background: Hsla,
    pub focus_background: Hsla,
    pub focused_border: Hsla,
}

#[derive(Clone)]
pub struct Colors {
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,
    pub idle: Hsla,
    pub online: Hsla,
    pub offline: Hsla,
    pub busy: Hsla,
    pub muted: Hsla,
}

impl Colors {
    fn new(red: Hsla, green: Hsla, yellow: Hsla, muted: Hsla) -> Self {
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
        }
    }
}

#[derive(Clone)]
pub struct ChatSidebarWidth {
    pub member: Pixels,
    pub search: Pixels,
    pub pinned: Pixels,
    pub members: Pixels,
}

#[derive(Clone)]
pub struct ServerColumn {
    pub icon_width: Pixels,
}

#[derive(Clone)]
pub struct Sidebar {
    pub width: Pixels,
    pub dm_icon_height: Pixels,
    pub channel_icon_height: Pixels,
}

#[derive(Clone)]
pub struct Header {
    pub height: Pixels,
    pub icon_size: Pixels,
}

impl Header {
    pub fn icon_padding(&self) -> Pixels {
        (self.height - self.icon_size) / 2.0
    }
}

#[derive(Clone)]
pub struct Chat {
    pub icon_size: Pixels,
    pub text_size: Pixels,
    pub small_icon_size: Pixels,
    pub small_text_size: Pixels,
    pub max_media_height: Pixels,
    pub max_media_width: Pixels,
}

#[derive(Clone)]
pub struct Structure {
    pub server_column: ServerColumn,
    pub chat_sidebar_width: ChatSidebarWidth,
    pub divider_width: Pixels,
    pub sidebar: Sidebar,
    pub header: Header,
    pub chat: Chat,
    pub outer_border_radius: Pixels,
    pub inner_border_radius: Pixels,
    pub smaller_border_radius: Pixels,
    pub font_size: Pixels,
    pub gap: Pixels,
    pub small_gap: Pixels,
}

impl Global for Structure {}

impl Structure {
    pub fn new() -> Self {
        let outer_border_radius = px(12.0);
        let gap = px(8.0);
        let small_gap = px(4.0);
        let inner_border_radius = outer_border_radius - small_gap;
        let smaller_border_radius = inner_border_radius - small_gap;

        Self {
            gap,
            small_gap,
            inner_border_radius,
            outer_border_radius,
            smaller_border_radius,
            font_size: px(16.0),
            divider_width: px(2.0),

            chat: Chat {
                icon_size: px(32.0),
                small_icon_size: px(18.0),
                text_size: px(16.0),
                small_text_size: px(12.0),
                max_media_height: px(500.0),
                max_media_width: px(500.0),
            },
            header: Header {
                height: px(50.0),
                icon_size: px(20.0),
            },
            sidebar: Sidebar {
                width: px(300.0),
                dm_icon_height: px(30.0),
                channel_icon_height: px(20.0),
            },
            server_column: ServerColumn {
                icon_width: px(40.0),
            },
            chat_sidebar_width: ChatSidebarWidth {
                member: px(320.0),
                search: px(480.0),
                pinned: px(480.0),
                members: px(240.0),
            },
        }
    }

    pub fn server_column_width(&self) -> Pixels {
        self.server_column.icon_width + 3.0 * self.gap
    }
}

#[derive(Clone)]
pub struct AppTheme {
    pub accent: Hsla,
    pub blur: Pixels,
    pub tile: TileTheme,
    pub text: Text,
    pub input: InputTheme,
    pub colors: Colors,
    pub pill_color: Hsla,
    pub solid_hover_bg: Hsla,
    pub solid_bg: Hsla,
}

impl Global for AppTheme {}

impl AppTheme {
    pub fn new() -> Self {
        let accent = hsla(0.53, 0.52, 0.52, 1.0);

        let muted_color = hsla(0.66, 0.15, 0.28, 1.0);

        let red = hsla(0.9462, 0.5569, 0.6725, 1.0);
        let green = hsla(0.3682, 0.5446, 0.6039, 1.0);
        let yellow = hsla(0.155, 0.786, 0.743, 1.0);

        let solid_bg = hsla(0.667, 0.211, 0.1, 1.0);
        let solid_hover_bg = solid_bg.lighten(0.8);

        Self {
            blur: px(30.0),
            solid_bg,
            solid_hover_bg,
            pill_color: white(),
            tile: TileTheme {
                background: hsla(0.66, 0.2, 0.1, 0.5),
                border: hsla(0.0, 0.0, 1.0, 0.175),
            },
            text: Text {
                muted: muted_color,
                dim: hsla(0.66, 0.15, 0.55, 1.0),
                normal: hsla(0.66, 0.15, 0.8, 1.0),
            },
            input: InputTheme {
                background: hsla(0.0, 0.0, 0.0, 0.2),
                focus_background: hsla(0.0, 0.0, 0.0, 0.4),
                focused_border: accent.darken(0.4),
            },
            colors: Colors::new(red, green, yellow, muted_color),
            accent,
        }
    }
}

pub trait ActiveAppTheme {
    fn app_theme(&self) -> &AppTheme;
}

impl ActiveAppTheme for App {
    fn app_theme(&self) -> &AppTheme {
        self.global::<AppTheme>()
    }
}

pub trait StructureExt {
    fn structure(&self) -> &Structure;
}

impl StructureExt for App {
    fn structure(&self) -> &Structure {
        self.global::<Structure>()
    }
}
