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
