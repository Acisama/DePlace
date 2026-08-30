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
    mouse::Interaction,
    widget::{
        self as w, Space, Stack,
        container::{self, Style as ContainerStyle},
        text,
    },
};
pub use matrix_sdk::{Client, Room};

pub use std::hash::Hash;
pub use std::sync::Arc;
