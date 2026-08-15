use gpui::Hsla;
use infer::{MatcherType, Type};

pub fn file_color(extentioninfer_type: Type) -> Hsla {
    match infer_type.matcher_type() {
        MatcherType::App => hsla(),
        MatcherType::Archive => hsla(),
        MatcherType::Audio => hsla(),
        MatcherType::Book => hsla(),
        MatcherType::Image => hsla(),
        MatcherType::Video => hsla(),
        MatcherType::Text => hsla(),
        MatcherType::Doc => hsla(),
        MatcherType::Font => hsla(),
        MatcherType::Custom => hsla(),
    }
}
