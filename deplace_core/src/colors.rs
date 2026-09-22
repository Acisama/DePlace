use csscolorparser::Color as CssColor;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct DePlaceColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl DePlaceColor {
    pub const TRANSPARENT: DePlaceColor = DePlaceColor {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    pub fn set_lightness(&self, lightness: f32) -> DePlaceColor {
        let [h, s, _, a] = self.to_hsla();
        CssColor::from_hsla(h, s, lightness, a).into()
    }

    pub fn set_alpha(&self, alpha: f32) -> DePlaceColor {
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
    pub fn to_iced(self) -> iced::Color {
        self.into()
    }

    pub fn darken(&self, amount: f32) -> DePlaceColor {
        let amount = amount.clamp(0.0, 1.0);
        DePlaceColor::from_rgba(
            self.r * (1.0 - amount),
            self.g * (1.0 - amount),
            self.b * (1.0 - amount),
            self.a,
        )
    }

    pub fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> DePlaceColor {
        DePlaceColor { r, g, b, a }
    }

    pub fn from_hsla(h: f32, s: f32, l: f32, a: f32) -> DePlaceColor {
        CssColor::from_hsla(h, s, l, a).into()
    }

    pub fn scale_alpha(&self, amount: f32) -> DePlaceColor {
        DePlaceColor::from_rgba(self.r, self.g, self.b, self.a * amount)
    }
}

impl From<&str> for DePlaceColor {
    fn from(string: &str) -> Self {
        let hash = Sha256::digest(string.as_bytes());
        let h = hash[0] as f32 / 255.0 * 360.0;
        CssColor::from_hsla(h, 0.9, 0.7, 1.0).into()
    }
}

impl From<CssColor> for DePlaceColor {
    fn from(color: CssColor) -> Self {
        DePlaceColor {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        }
    }
}

#[cfg(feature = "desktop")]
impl From<&DePlaceColor> for gpui::Hsla {
    fn from(val: &DePlaceColor) -> Self {
        let [h, s, l, a] = val.to_hsla();
        gpui::hsla(h / 360.0, s, l, a)
    }
}

#[cfg(feature = "iced_desktop")]
impl From<DePlaceColor> for iced::Color {
    fn from(val: DePlaceColor) -> Self {
        iced::Color::from_rgba(val.r, val.g, val.b, val.a)
    }
}

#[cfg(feature = "iced_desktop")]
impl From<DePlaceColor> for iced::Background {
    fn from(val: DePlaceColor) -> Self {
        iced::Background::Color(iced::Color::from_rgba(val.r, val.g, val.b, val.a))
    }
}
