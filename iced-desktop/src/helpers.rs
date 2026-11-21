use iced::{Background, Border, Color, Padding, Size, Vector, border::Radius, padding};

pub fn blend_color(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);

    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

pub fn blend_border(from: Border, to: Border, t: f32) -> Border {
    Border {
        color: blend_color(from.color, to.color, t),
        width: from.width + (to.width - from.width) * t,
        radius: Radius {
            top_left: from.radius.top_left + (to.radius.top_left - from.radius.top_left) * t,
            top_right: from.radius.top_right + (to.radius.top_right - from.radius.top_right) * t,
            bottom_right: from.radius.bottom_right
                + (to.radius.bottom_right - from.radius.bottom_right) * t,
            bottom_left: from.radius.bottom_left
                + (to.radius.bottom_left - from.radius.bottom_left) * t,
        },
    }
}

/// Blends solid-color backgrounds channel-wise, treating `None` as fully
/// transparent so fades in/out smoothly instead of popping.
pub fn blend_background(
    from: Option<Background>,
    to: Option<Background>,
    t: f32,
) -> Option<Background> {
    match (from, to) {
        (Some(Background::Color(from)), Some(Background::Color(to))) => {
            Some(Background::Color(blend_color(from, to, t)))
        }
        (None, Some(Background::Color(to))) => {
            Some(Background::Color(blend_color(Color::TRANSPARENT, to, t)))
        }
        (Some(Background::Color(from)), None) => {
            Some(Background::Color(blend_color(from, Color::TRANSPARENT, t)))
        }
        (from, to) => {
            if t < 0.5 {
                from
            } else {
                to
            }
        }
    }
}

pub fn lerp_vector(from: Vector, to: Vector, t: f32) -> Vector {
    Vector::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
}

pub fn lerp_size(from: Size, to: Size, t: f32) -> Size {
    Size::new(
        from.width + (to.width - from.width) * t,
        from.height + (to.height - from.height) * t,
    )
}

pub fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Creates a padding with horizontal padding equal to the given value and vertical padding equal to half.
pub fn one_and_half_padding(padding: f32) -> Padding {
    padding::horizontal(padding).vertical(padding / 2.0)
}
