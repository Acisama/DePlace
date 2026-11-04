pub use crate::EMPTY_WIDGET;
pub use crate::components::{
    ColoredRounded, CornerContent, Explainable, GenericState, IcedColorExt, IcedWidget,
    MapElementExt, StatusExt, UI_FONT,
    animated_button::animated_button,
    close_button, corner_badge,
    help_mode::HelpKey,
    help_mode::HelpView,
    help_mode::{Caption, HelpOverMessage, HelpState, help, help_root},
    on_appear, pan,
    pie_progress::pie_progress,
    profile::{
        MemberRenderExt, ProfileRenderExt, RoomRenderExt, loading_icon, render_avatar,
        render_loading_name, render_name, render_presence_badge, render_unknown_name, text_icon,
        unknown_icon,
    },
    text_input, themed_scrollable,
    tile_background::floating_tile,
    tooltip::{themed_tooltip_content, themed_tooltip_text},
    weighted_text,
};
pub use deplace_core::{
    Avatar, ProfileLike,
    colors::DePlaceColor,
    get_change,
    helpers::{DisplayString, EventChange, get_current_and_prev},
    member::{Member, store::ProfileStore},
    room::{DePlaceRoom, store::RoomStore},
    settings::Settings,
    state::{
        AppState,
        cache::{
            CacheLoadingExt, CacheLoadingWithKeyExt, ImageCache, MediaLoaded, MediaState,
            NeedsMedia, ThumbnailCache, VideoCache,
        },
        roles::ExtraHash,
    },
    structure::Structure,
    theme::Theme,
};
use iced::advanced::Widget;
pub use iced::{
    Border, Color, Element, Font,
    Length::Fill,
    Padding, Point, Rectangle, Renderer, Shadow, Size, Subscription, Task, Theme as IcedTheme,
    Widget as _, border,
    font::Weight,
    mouse::{self, Interaction},
    padding,
    widget::{
        self as w, Canvas, Column, MouseArea, Row, Space, Stack, Text,
        button::{self, Style as ButtonStyle},
        canvas,
        container::{self, Container, Style as ContainerStyle},
        image::Handle as ImageHandle,
        svg::{self, Style as SvgStyle},
        text::{self, Style as TextStyle},
    },
    window,
};
pub use indexmap::IndexMap;
pub use matrix_sdk::{
    Client,
    ruma::{
        MxcUri, OwnedEventId, OwnedMxcUri, OwnedRoomId, OwnedUserId, RoomId, UserId,
        presence::PresenceState,
    },
};
use std::cell::Cell;
pub use std::hash::Hash;
pub use std::sync::Arc;
pub use tokio::sync::watch::Receiver;

thread_local! {
    static THEME: Cell<Theme> = Cell::new(Theme::default());
    static STRUCTURE: Cell<Structure> = Cell::new(Structure::default());
    static HELP: Cell<HelpState<HelpKey>> = Cell::new(HelpState::default());
    static STYLE_GEN: Cell<u64> = const { Cell::new(0) };
}

pub fn theme() -> Theme {
    THEME.get()
}
pub fn structure() -> Structure {
    STRUCTURE.get()
}
pub fn help_state() -> HelpState<HelpKey> {
    HELP.get()
}
pub fn style_gen() -> u64 {
    STYLE_GEN.get()
}

pub fn set_theme(t: Theme) {
    THEME.set(t);
    STYLE_GEN.set(STYLE_GEN.get() + 1);
}

pub fn set_structure(s: Structure) {
    STRUCTURE.set(s);
    STYLE_GEN.set(STYLE_GEN.get() + 1);
}

pub fn set_help_state(h: HelpState<HelpKey>) {
    HELP.set(h);
}

pub fn lazy<'a, Message, Dep, View>(
    dep: Dep,
    view: impl Fn(&Dep) -> View + 'a,
) -> Element<'a, Message>
where
    Message: 'static,
    Dep: Hash + 'a,
    View: Into<Element<'static, Message>> + 'static,
{
    w::lazy((dep, style_gen(), help_state()), move |(dep, ..)| {
        view(dep).into()
    })
    .boxed()
}

pub fn lazy_view<'a, Message, Dep, Action, OuterMessage>(
    dep: Dep,
    map_message: impl Fn(Message) -> OuterMessage + 'a,
) -> Element<'a, OuterMessage>
where
    Message: 'static,
    OuterMessage: 'static,
    Dep: Hash + IcedWidget<Message, Action> + 'a,
{
    w::lazy((dep, style_gen(), help_state()), |(dep, ..)| dep.view()).map_el(map_message)
}

pub trait DePlaceWidget<Message>: Widget<Message, iced::Theme, Renderer> {}

impl<Message, T> DePlaceWidget<Message> for T where T: Widget<Message, iced::Theme, Renderer> {}
