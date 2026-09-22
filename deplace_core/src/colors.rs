use csscolorparser::Color as CssColor;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn set_lightness(&self, lightness: f32) -> Color {
        let [h, s, _, a] = self.to_hsla();
        CssColor::from_hsla(h, s, lightness, a).into()
    }

    pub fn set_alpha(&self, alpha: f32) -> Color {
        let [h, s, l, _] = self.to_hsla();
        CssColor::from_hsla(h, s, l, alpha).into()
    }

    pub fn to_hsla(&self) -> [f32; 4] {
        CssColor::new(self.r, self.g, self.b, self.a).to_hsla()
    }

    #[cfg(feature = "desktop")]
    pub fn to_gpui(&self) -> gpui::Hsla {
        self.into()
    }

    #[cfg(feature = "iced_desktop")]
    pub fn to_iced(&self) -> iced::Color {
        self.into()
    }

    pub fn darken(&self, amount: f32) -> Color {
        let amount = amount.clamp(0.0, 1.0);
        Color::from_rgba(
            self.r * (1.0 - amount),
            self.g * (1.0 - amount),
            self.b * (1.0 - amount),
            self.a,
        )
    }

    pub fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    pub fn from_hsla(h: f32, s: f32, l: f32, a: f32) -> Color {
        CssColor::from_hsla(h, s, l, a).into()
    }
}

impl From<&str> for Color {
    fn from(string: &str) -> Self {
        let hash = Sha256::digest(string.as_bytes());
        let h = hash[0] as f32 / 255.0 * 360.0;
        CssColor::from_hsla(h, 0.9, 0.7, 1.0).into()
    }
}

impl From<CssColor> for Color {
    fn from(color: CssColor) -> Self {
        Color {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        }
    }
}

#[cfg(feature = "desktop")]
impl From<&Color> for gpui::Hsla {
    fn from(val: &Color) -> Self {
        let [h, s, l, a] = val.to_hsla();
        gpui::hsla(h / 360.0, s, l, a)
    }
}

#[cfg(feature = "iced_desktop")]
impl From<&Color> for iced::Color {
    fn from(val: &Color) -> Self {
        iced::Color::from_rgba(val.r, val.g, val.b, val.a)
    }
}
