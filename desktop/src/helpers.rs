use gpui::{Hsla, hsla};
use infer::{MatcherType, Type};

pub fn file_color(infer_type: Type) -> Hsla {
    match infer_type.matcher_type() {
        MatcherType::App => hsla(0.60, 0.55, 0.55, 1.0),
        MatcherType::Archive => hsla(0.10, 0.70, 0.55, 1.0),
        MatcherType::Audio => hsla(0.90, 0.55, 0.60, 1.0),
        MatcherType::Book => hsla(0.08, 0.45, 0.40, 1.0),
        MatcherType::Image => hsla(0.35, 0.55, 0.50, 1.0),
        MatcherType::Video => hsla(0.78, 0.55, 0.55, 1.0),
        MatcherType::Text => hsla(0.60, 0.10, 0.75, 1.0),
        MatcherType::Doc => hsla(0.58, 0.65, 0.45, 1.0),
        MatcherType::Font => hsla(0.50, 0.55, 0.50, 1.0),
        MatcherType::Custom => hsla(0.0, 0.0, 0.55, 1.0),
    }
}
