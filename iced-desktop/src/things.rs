use std::time::Duration;

use iced::Color;

fn hsla(h: f32, s: f32, l: f32, a: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let cm = c + m;
    let xm = x + m;

    let (r, g, b) = match (h * 6.0).floor() as i32 {
        0 | 6 => (cm, xm, m),
        1 => (xm, cm, m),
        2 => (m, cm, xm),
        3 => (m, xm, cm),
        4 => (xm, m, cm),
        _ => (cm, m, xm),
    };

    Color::from_rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a)
}

trait ColorExt {
    fn alpha(&self, a: f32) -> Self;
    fn darken(&self, amount: f32) -> Self;
    fn blend(&self, other: Color) -> Self;
}

impl ColorExt for Color {
    fn alpha(&self, a: f32) -> Self {
        Color {
            a: a.clamp(0.0, 1.0),
            ..*self
        }
    }

    fn darken(&self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        Color {
            r: self.r * (1.0 - amount),
            g: self.g * (1.0 - amount),
            b: self.b * (1.0 - amount),
            a: self.a,
        }
    }

    fn blend(&self, other: Color) -> Self {
        if other.a >= 1.0 {
            other
        } else if other.a <= 0.0 {
            *self
        } else {
            Color {
                r: self.r * (1.0 - other.a) + other.r * other.a,
                g: self.g * (1.0 - other.a) + other.g * other.a,
                b: self.b * (1.0 - other.a) + other.b * other.a,
                a: self.a,
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct Text {
    pub muted: Color,
    pub dim: Color,
    pub normal: Color,
}

#[derive(Clone, Copy)]
pub struct InputTheme {
    pub background: Color,
    pub focus_background: Color,
    pub focused_border: Color,
}

#[derive(Clone, Copy)]
pub struct Colors {
    pub _red: Color,
    pub _green: Color,
    pub _yellow: Color,
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
            _red: red,
            _green: green,
            _yellow: yellow,
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

#[derive(Clone, Copy)]
pub struct ChatSidebarWidth {
    pub member: f32,
    pub search: f32,
    pub pinned: f32,
    pub members: f32,
}

#[derive(Clone, Copy)]
pub struct ServerColumn {
    pub icon_size: f32,
}

#[derive(Clone, Copy)]
pub struct Sidebar {
    pub width: f32,
    pub dm_icon_height: f32,
    pub channel_icon_height: f32,
}

#[derive(Clone, Copy)]
pub struct Header {
    pub height: f32,
    pub icon_size: f32,
}

impl Header {
    pub fn icon_padding(&self) -> f32 {
        (self.height - self.icon_size) / 2.0
    }
}

#[derive(Clone, Copy)]
pub struct Chat {
    pub icon_size: f32,
    pub text_size: f32,
    pub small_icon_size: f32,
    pub small_text_size: f32,
    pub max_media_height: f32,
    pub max_media_width: f32,
    pub input_height: f32,
    pub attachment_preview_dimensions: (f32, f32),
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub section_column_width: f32,
    pub full_width: f32,
    pub full_height: f32,
    pub checkbox_width: f32,
    pub checkbox_height: f32,
    pub dropdown_width: f32,
}

#[derive(Clone, Copy)]
pub struct Authentification {
    pub width: f32,
}

#[derive(Clone, Copy)]
pub struct Structure {
    pub server_column: ServerColumn,
    pub chat_sidebar_width: ChatSidebarWidth,
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
    pub settings: Settings,
    pub authentification: Authentification,
}

impl Structure {
    pub fn new() -> Self {
        let outer_border_radius = 18.0;
        let gap = 12.0;
        let small_gap = 8.0;
        let inner_border_radius = outer_border_radius - small_gap;
        let smaller_border_radius = inner_border_radius - small_gap;

        Self {
            gap,
            small_gap,
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
            },
            header: Header {
                height: 50.0,
                icon_size: 20.0,
            },
            sidebar: Sidebar {
                width: 300.0,
                dm_icon_height: 30.0,
                channel_icon_height: 20.0,
            },
            server_column: ServerColumn { icon_size: 40.0 },
            chat_sidebar_width: ChatSidebarWidth {
                member: 320.0,
                search: 480.0,
                pinned: 480.0,
                members: 240.0,
            },
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

#[derive(Clone, Copy)]
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

    pub _hover_animation_duration: Duration,
}

impl Theme {
    pub fn new() -> Self {
        let accent = hsla(0.53, 0.52, 0.52, 1.0);

        let muted_color = hsla(0.66, 0.15, 0.3, 1.0);

        let red = hsla(0.9462, 0.5569, 0.6725, 1.0);
        let green = hsla(0.3682, 0.5446, 0.6039, 1.0);
        let yellow = hsla(0.155, 0.786, 0.743, 1.0);

        let solid_bg = hsla(0.6667, 0.5000, 0.0314, 1.0);
        let solid_hover_bg = hsla(0.6667, 0.2105, 0.1490, 1.0);

        Self {
            blur: 20.0,
            solid_bg,
            solid_hover_bg,
            pill_color: Color::WHITE,
            border: hsla(0.0, 0.0, 1.0, 0.175),
            background: hsla(0.6667, 0.5000, 0.0314, 1.0),
            text: Text {
                muted: muted_color,
                dim: hsla(0.66, 0.15, 0.4, 1.0),
                normal: hsla(0.66, 0.15, 0.7, 1.0),
            },
            input: InputTheme {
                background: hsla(0.0, 0.0, 0.0, 0.2),
                focus_background: hsla(0.0, 0.0, 0.0, 0.4),
                focused_border: accent.darken(0.4),
            },
            colors: Colors::new(red, green, yellow, muted_color, hsla(0.0, 1.0, 0.7, 1.0)),
            accent,

            _hover_animation_duration: Duration::from_millis(150),
        }
    }

    pub fn accent_bg(&self) -> Color {
        self.solid_bg.blend(self.accent.alpha(0.1))
    }
}
