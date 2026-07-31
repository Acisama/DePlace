use gpui::{App, Global, Hsla, Pixels, hsla, px, white};
use gpui_component::Colorize;

#[derive(Clone)]
pub struct TileTheme {
    pub background: Hsla,
    pub border: Hsla,
    pub border_thickness: Pixels,
    pub border_radius: Pixels,
    pub gap: Pixels,
}

#[derive(Clone)]
pub struct TextTheme {
    pub font_size: Pixels,
    pub muted: Hsla,
    pub dim: Hsla,
    pub normal: Hsla,
}

#[derive(Clone)]
pub struct InputTheme {
    pub background: Hsla,
    pub focus_background: Hsla,
    pub focused_border: Hsla,
    pub padding: Pixels,
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
pub struct Structure {
    pub header_height: Pixels,
    pub sidebar_width: Pixels,
    pub server_column: ServerColumn,
    pub chat_sidebar_width: ChatSidebarWidth,
    pub divider_width: Pixels,
}

#[derive(Clone)]
pub struct AppTheme {
    pub gap: Pixels,
    pub background: Hsla,
    pub small_gap: Pixels,
    pub accent: Hsla,
    pub tile: TileTheme,
    pub text: TextTheme,
    pub input: InputTheme,
    pub colors: Colors,
    pub structure: Structure,
    pub pill_color: Hsla,
}

impl Global for AppTheme {}

impl AppTheme {
    pub fn new() -> Self {
        let accent = hsla(0.53, 0.52, 0.52, 1.0);

        let muted_color = hsla(0.66, 0.15, 0.25, 1.0);

        let red = hsla(0.9462, 0.5569, 0.6725, 1.0);
        let green = hsla(0.3682, 0.5446, 0.6039, 1.0);
        let yellow = hsla(0.155, 0.786, 0.743, 1.0);

        Self {
            gap: px(8.0),
            background: hsla(0.0, 0.0, 0.1, 1.0),
            small_gap: px(2.0),
            pill_color: white(),
            tile: TileTheme {
                background: hsla(0.66, 0.2, 0.1, 0.5),
                border: hsla(0.0, 0.0, 0.2, 1.0),
                border_thickness: px(1.0),
                border_radius: px(12.0),
                gap: px(8.0),
            },
            text: TextTheme {
                font_size: px(16.0),
                muted: muted_color,
                dim: hsla(0.66, 0.15, 0.55, 1.0),
                normal: hsla(0.66, 0.15, 0.8, 1.0),
            },
            input: InputTheme {
                background: hsla(0.0, 0.0, 0.0, 0.2),
                focus_background: hsla(0.0, 0.0, 0.0, 0.4),
                focused_border: accent.darken(0.4),
                padding: px(8.0),
            },
            colors: Colors::new(red, green, yellow, muted_color),
            accent,
            structure: Structure {
                header_height: px(50.0),
                sidebar_width: px(300.0),
                server_column: ServerColumn {
                    icon_width: px(40.0),
                },
                chat_sidebar_width: ChatSidebarWidth {
                    member: px(320.0),
                    search: px(480.0),
                    pinned: px(480.0),
                    members: px(240.0),
                },
                divider_width: px(2.0),
            },
        }
    }

    pub fn server_column_width(&self) -> Pixels {
        self.structure.server_column.icon_width + 3.0 * self.gap
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
