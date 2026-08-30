pub use crate::{
    components::{
        GenericState, IconExt, NeedsAvatarExt, floating_tile, on_appear, text_input, weighted_text,
    },
    things::{Structure, Theme},
};
pub use deplace_core::{NameExt, settings::Settings, state::AppState};
pub use iced::{
    Border, Color, Element,
    Length::Fill,
    Shadow, Task,
    widget::{self as w, Space, container, text},
};
pub use matrix_sdk::{Client, Room};

pub use std::hash::Hash;
