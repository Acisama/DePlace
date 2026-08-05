use csscolorparser::Color as CssColor;
use matrix_sdk::{Room, room::RoomMember};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Color(CssColor);

impl Color {
    pub fn get(&self) -> CssColor {
        self.0.clone()
    }

    pub fn set_lightness(&self, lightness: f32) -> Color {
        let [h, s, _, a] = self.0.to_hsla();
        Color(CssColor::from_hsla(h, s, lightness, a))
    }

    pub fn set_alpha(&self, alpha: f32) -> Color {
        let [h, s, l, _] = self.0.to_hsla();
        Color(CssColor::from_hsla(h, s, l, alpha))
    }

    pub fn to_hsla(&self) -> [f32; 4] {
        self.0.to_hsla()
    }

    #[cfg(feature = "desktop")]
    pub fn to_gpui(&self) -> gpui::Hsla {
        let [h, s, l, a] = self.0.to_hsla();
        gpui::hsla(h / 360.0, s, l, a)
    }
}

impl From<&str> for Color {
    fn from(string: &str) -> Self {
        let hash = Sha256::digest(string.as_bytes());
        let h = hash[0] as f32 / 255.0 * 360.0;
        Color(CssColor::from_hsla(h, 0.9, 0.7, 1.0))
    }
}

impl From<CssColor> for Color {
    fn from(color: CssColor) -> Self {
        Color(color)
    }
}

pub trait ColorExt {
    fn color(&self) -> Color;
}

impl ColorExt for RoomMember {
    fn color(&self) -> Color {
        self.user_id().as_str().into()
    }
}

impl ColorExt for Room {
    fn color(&self) -> Color {
        self.room_id().as_str().into()
    }
}

#[cfg(feature = "desktop")]
impl From<Color> for gpui::Hsla {
    fn from(val: Color) -> Self {
        let [h, s, l, a] = val.0.to_hsla();
        gpui::hsla(h / 360.0, s, l, a)
    }
}
