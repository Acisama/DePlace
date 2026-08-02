use std::sync::Arc;

use deplace_core::{matrix_api::account_data::set_account_data, state::AppState};
use gpui::{
    AppContext, Context, Empty, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, Pixels, Render, Styled, Window, div, prelude::FluentBuilder,
};
use gpui_component::StyledExt;
use matrix_sdk::{Room, ruma::OwnedUserId};

use crate::{
    components::{
        AvatarCache, MediaCache, chat::ChatView, dm_list::DmListView, floating_tile,
        header::HeaderView, quick_select, server_list::ServerListView, sidebar::SidebarView,
    },
    theme::{ActiveAppTheme, Structure},
};

pub struct HomeView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    focus: FocusHandle,

    state: AppState,
    chat_sidebar: Option<ChatSidebar>,
    server_list: Entity<ServerListView>,
    header: Entity<HeaderView>,
    dm_list: Entity<DmListView>,
    sidebar: Entity<SidebarView>,
    chat: Entity<ChatView>,

    overlay: Overlay,
}

#[derive(Debug)]
enum Overlay {
    None,
    Settings,
    QuickSelect(Entity<quick_select::QuickSelect>),
}

#[derive(Clone)]
enum ChatSidebar {
    Members,
    Search,
    Pinned,
    Member(OwnedUserId),
}

pub struct ActiveServerChange(Option<Room>);

impl ActiveServerChange {
    pub fn new(room: Option<Room>) -> Self {
        Self(room)
    }

    pub fn room(&self) -> Option<Room> {
        self.0.clone()
    }
}

pub struct ActiveRoomChange(Option<Room>);

impl ActiveRoomChange {
    pub fn new(room: Option<Room>) -> Self {
        Self(room)
    }

    pub fn room(&self) -> Option<Room> {
        self.0.clone()
    }
}

impl ChatSidebar {
    pub fn get_width(&self, structure: &Structure) -> Pixels {
        match self {
            ChatSidebar::Member(_) => structure.chat_sidebar_width.member,
            ChatSidebar::Members => structure.chat_sidebar_width.members,
            ChatSidebar::Search => structure.chat_sidebar_width.search,
            ChatSidebar::Pinned => structure.chat_sidebar_width.pinned,
        }
    }
}

impl HomeView {
    pub fn new(
        tokio_rt: Arc<tokio::runtime::Runtime>,
        state: AppState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let avatar_cache: AvatarCache = MediaCache::new(state.client.clone(), tokio_rt.clone());

        let server_list =
            cx.new(|cx| ServerListView::new(&state, cx, tokio_rt.clone(), avatar_cache.clone()));
        let dm_list = cx.new(|cx| DmListView::new(&state, cx));
        let header =
            cx.new(|cx| HeaderView::new(&state, cx, tokio_rt.clone(), avatar_cache.clone()));
        let sidebar =
            cx.new(|cx| SidebarView::new(&state, cx, tokio_rt.clone(), avatar_cache.clone()));
        let chat = cx.new(|cx| ChatView::new(&state, cx, tokio_rt.clone(), avatar_cache.clone()));

        cx.subscribe_in(
            &server_list,
            window,
            move |this: &mut HomeView, _child, event: &ActiveServerChange, &mut _, &mut _| {
                let room = event.room();
                let server_id = room.as_ref().map(|r| r.room_id());

                this.state.set_active_server(room.clone());
                let mut breadcrumbs = this.state.breadcrumbs.clone();
                let new_room_id = if let Some(room_id) = server_id {
                    breadcrumbs
                        .last_space_ids
                        .get(room_id)
                        .cloned()
                        .or_else(|| {
                            let mut children: Vec<(Room, Option<String>)> = this
                                .state
                                .parent_to_children()
                                .borrow()
                                .get(room_id)
                                .cloned()
                                .unwrap_or_default()
                                .values()
                                .cloned()
                                .collect();

                            children
                                .sort_by_key(|(r, o)| o.clone().unwrap_or(r.room_id().to_string()));
                            children.first().map(|(r, _)| r.room_id().to_owned())
                        })
                } else {
                    breadcrumbs.last_dm_id.clone().or_else(|| {
                        this.state
                            .dm_rooms()
                            .borrow()
                            .values()
                            .next()
                            .map(|r| r.room_id().to_owned())
                    })
                };

                let new_room = if let Some(id) = new_room_id {
                    if let Some(server_id) = server_id {
                        breadcrumbs
                            .last_space_ids
                            .insert(server_id.to_owned(), id.clone());
                    } else {
                        breadcrumbs.last_dm_id = Some(id.clone());
                    };

                    breadcrumbs.recent_rooms.insert(0, id.clone());
                    this.state.client.get_room(&id)
                } else {
                    None
                };

                if breadcrumbs.recent_rooms.len() > 10 {
                    breadcrumbs.recent_rooms.truncate(10);
                }

                this.state.set_active_room(new_room);
                this.state.breadcrumbs = breadcrumbs.clone();

                let client = this.state.client.clone();
                this.tokio_rt.spawn(async move {
                    set_account_data(&client, breadcrumbs).await;
                });
            },
        )
        .detach();

        cx.subscribe_in(
            &sidebar,
            window,
            move |this: &mut HomeView, _child, event: &ActiveRoomChange, _, _| {
                let room = event.room();
                this.state.set_active_room(room.clone());

                if let Some(room) = room {
                    let mut breadcrumbs = this.state.breadcrumbs.clone();
                    breadcrumbs
                        .recent_rooms
                        .insert(0, room.room_id().to_owned());

                    if breadcrumbs.recent_rooms.len() > 10 {
                        breadcrumbs.recent_rooms.truncate(10);
                    }

                    this.state.breadcrumbs = breadcrumbs.clone();
                    let client = this.state.client.clone();
                    this.tokio_rt
                        .spawn(async move { set_account_data(&client, breadcrumbs).await });
                }
            },
        )
        .detach();

        Self {
            server_list,
            dm_list,
            tokio_rt,
            state,
            header,
            sidebar,
            chat,
            chat_sidebar: Some(ChatSidebar::Members),
            focus: focus_handle,
            overlay: Overlay::None,
        }
    }
}

impl Focusable for HomeView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HomeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let padding = theme.gap;
        let structure = &theme.structure;

        div()
            .track_focus(&self.focus)
            .id("home-view")
            .key_context("Home")
            .on_action(
                cx.listener(|this, _action: &quick_select::Open, window, cx| {
                    tracing::debug!("Opening quick select");

                    let quick_select = cx.new(|cx| quick_select::QuickSelect::new(window, cx));

                    cx.subscribe_in(
                        &quick_select,
                        window,
                        |this: &mut HomeView, _child, _event: &quick_select::Close, window, cx| {
                            this.overlay = Overlay::None;
                            window.focus(&this.focus, cx);
                            cx.notify()
                        },
                    )
                    .detach();

                    this.overlay = Overlay::QuickSelect(quick_select);
                    cx.notify();
                }),
            )
            .flex()
            .flex_row()
            .paddings(padding)
            .gap(padding)
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(padding)
                    .child(
                        floating_tile(theme)
                            .w(theme.server_column_width())
                            .child(self.server_list.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(structure.sidebar.width)
                            .gap(padding)
                            .child(
                                floating_tile(theme)
                                    .flex_grow_1()
                                    .child(self.sidebar.clone()),
                            )
                            .child(floating_tile(theme).h(structure.header.height)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(padding)
                    .size_full()
                    .child(
                        floating_tile(theme)
                            .h(structure.header.height)
                            .child(self.header.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(padding)
                            .size_full()
                            .child(
                                floating_tile(theme)
                                    .flex_grow_1()
                                    .h_full()
                                    .child(self.chat.clone()),
                            )
                            .when_some(self.chat_sidebar.clone(), |el, sidebar| {
                                el.child(
                                    floating_tile(theme)
                                        .w(sidebar.get_width(structure))
                                        .h_full(),
                                )
                            }),
                    ),
            )
            .child(match &self.overlay {
                Overlay::None => Empty.into_any_element(),
                overlay => div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::rgba(0x00000080))
                    .child(match overlay {
                        Overlay::QuickSelect(ent) => ent.clone().into_any_element(),
                        Overlay::Settings => div().into_any_element(),
                        Overlay::None => div().into_any_element(),
                    })
                    .into_any_element(),
            })
    }
}
