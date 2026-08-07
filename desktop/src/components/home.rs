use std::sync::Arc;

use deplace_core::{
    get_other_member,
    helpers::RoomPlaceholderExt,
    matrix_api::{account_data::set_account_data, messages::send_message},
    state::{AppState, MembershipMap},
};
use gpui::{
    AppContext, Context, Empty, Entity, FocusHandle, Focusable, FollowMode, InteractiveElement,
    IntoElement, KeyContext, KeyDownEvent, Keystroke, ParentElement, Pixels, Render,
    StyleRefinement, Styled, Window, actions, div, prelude::FluentBuilder,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};
use macros::tailwind_div;
use matrix_sdk::{
    Room,
    room::RoomMember,
    ruma::{OwnedUserId, UserId},
};
use tokio::sync::watch::Receiver;

use crate::{
    components::{
        AvatarCache, CustomStyles,
        cache::ThumbnailCache,
        chat::{
            ChatView, FocusInput, FocusInputWithKey, FocusNext, FocusPrevious, SendMessage,
            UnfocusInput,
        },
        floating_tile,
        header::HeaderView,
        quick_select,
        server_list::ServerListView,
        sidebar::SidebarView,
    },
    theme::{ActiveAppTheme, Structure, StructureExt},
    watch_bridge::notify_on_change,
};

pub struct HomeView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    focus: FocusHandle,

    state: AppState,
    chat_sidebar: Option<ChatSidebar>,
    server_list: Entity<ServerListView>,
    header: Entity<HeaderView>,
    sidebar: Entity<SidebarView>,
    chat: Entity<ChatView>,
    chat_input: Entity<ChatInputBar>,

    active_room: Receiver<Option<Room>>,
    membership_map: Receiver<MembershipMap>,
    own_id: OwnedUserId,

    overlay: Overlay,

    vim_mode: bool,
}

actions!(home, [ToggleVimMode]);

#[derive(Debug)]
enum Overlay {
    None,
    Settings,
    QuickSelect(Entity<quick_select::QuickSelect>),
}

#[derive(Clone)]
enum ChatSidebar {
    Members(Room),
    Search,
    Pinned,
    Member(RoomMember),
}

impl PartialEq for ChatSidebar {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (ChatSidebar::Members(_), ChatSidebar::Members(_))
                | (ChatSidebar::Search, ChatSidebar::Search)
                | (ChatSidebar::Pinned, ChatSidebar::Pinned)
                | (ChatSidebar::Member(_), ChatSidebar::Member(_))
        )
    }
}

impl ChatSidebar {
    fn from_active_room(room: Option<Room>, own_id: &UserId, map: MembershipMap) -> Option<Self> {
        let room = room?;

        if room.is_dm()
            && let Some(member) = get_other_member(own_id, &map, room.room_id())
        {
            Some(ChatSidebar::Member(member))
        } else {
            Some(ChatSidebar::Members(room.clone()))
        }
    }
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
            ChatSidebar::Members(_) => structure.chat_sidebar_width.members,
            ChatSidebar::Search => structure.chat_sidebar_width.search,
            ChatSidebar::Pinned => structure.chat_sidebar_width.pinned,
        }
    }
}

pub struct ChatInputBar {
    chat_input: Entity<InputState>,
    active_room: Receiver<Option<Room>>,
    membership_map: Receiver<MembershipMap>,
}

impl ChatInputBar {
    pub fn new(state: &AppState, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let active_room = state.active_room();
        let membership_map = state.membership_map();

        let chat_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(1, 10)
                .placeholder(
                    active_room
                        .borrow()
                        .clone()
                        .get_input_placeholder(&membership_map.borrow()),
                )
        });

        let view = Self {
            chat_input,
            active_room: active_room.clone(),
            membership_map: membership_map.clone(),
        };

        cx.spawn({
            let mut active_room = active_room.clone();
            async move |this, cx| {
                while active_room.changed().await.is_ok() {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        this.update_in(cx, |view, window, cx| view.update_placeholder(window, cx))
                    })) {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => break,
                        Err(e) => {
                            tracing::error!(
                                "Panic while updating chat input placeholder, will keep listening for room changes: {:?}",
                                e
                            );
                        }
                    }
                }
            }
        })
        .detach();

        cx.spawn({
            let mut map = membership_map.clone();
            async move |this, cx| {
                while map.changed().await.is_ok() {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        this.update_in(cx, |view, window, cx| view.update_placeholder(window, cx))
                    })) {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => break,
                        Err(e) => {
                            tracing::error!(
                                "Panic while updating chat input placeholder, will keep listening for membership changes: {:?}",
                                e
                            );
                        }
                    }
                }
            }
        })
        .detach();

        view
    }

    fn update_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let active_room = self.active_room.borrow().clone();
        let map = self.membership_map.borrow().clone();

        self.chat_input.update(cx, |input, cx| {
            input.set_placeholder(active_room.get_input_placeholder(&map), window, cx)
        });
    }
}

impl Focusable for ChatInputBar {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.chat_input.focus_handle(cx)
    }
}

pub struct SendMessageEvent {
    pub text: String,
}

impl gpui::EventEmitter<SendMessageEvent> for ChatInputBar {}

impl Render for ChatInputBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        let input_focus_handle = self.chat_input.read(cx).focus_handle(cx);
        let input_focused = input_focus_handle.is_focused(window);

        let (input_bg, input_border) = if input_focused {
            (theme.input.focus_background, theme.input.focused_border)
        } else {
            (theme.input.background, theme.tile.border)
        };

        tailwind_div!(
            min_h(structure.header.height),
            flex,
            flex_row,
            items_center,
            w_full,
            rounded(structure.inner_border_radius),
            text_size(structure.chat.text_size),
            border_1,
            border_color(input_border),
            bg(input_bg)
        )
        .track_focus(&input_focus_handle)
        .on_action(cx.listener(|this, _event: &SendMessage, _window, cx| {
            let text = this.chat_input.read(cx).text().to_string();
            if !text.trim().is_empty() {
                cx.emit(SendMessageEvent { text });
            }
            this.chat_input.update(cx, |input, cx| {
                input.set_value("", _window, cx);
            })
        }))
        .child(
            Input::new(&self.chat_input)
                .bg_transparent()
                .border_transparent()
                .text_color(theme.text.normal),
        )
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
        let avatar_cache: AvatarCache = AvatarCache::new(state.client.clone(), tokio_rt.clone());
        let image_cache: ThumbnailCache =
            ThumbnailCache::new(state.client.clone(), tokio_rt.clone());

        let server_list =
            cx.new(|cx| ServerListView::new(&state, cx, tokio_rt.clone(), avatar_cache.clone()));
        let header =
            cx.new(|cx| HeaderView::new(&state, cx, tokio_rt.clone(), avatar_cache.clone()));
        let sidebar =
            cx.new(|cx| SidebarView::new(&state, cx, tokio_rt.clone(), avatar_cache.clone()));
        let chat = cx.new(|cx| {
            ChatView::new(
                &state,
                cx,
                tokio_rt.clone(),
                avatar_cache.clone(),
                image_cache.clone(),
            )
        });
        let chat_input = cx.new(|cx| ChatInputBar::new(&state, window, cx));

        let active_room = state.active_room();
        let membership_map = state.membership_map();
        let own_id = state.user_device.user_id.clone();

        notify_on_change(active_room.clone(), cx);
        notify_on_change(membership_map.clone(), cx);

        cx.subscribe_in(
            &server_list,
            window,
            move |this: &mut HomeView,
                  _child,
                  event: &ActiveServerChange,
                  &mut _,
                  cx: &mut Context<Self>| {
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

                this.update_chat_sidebar(cx);
            },
        )
        .detach();

        cx.subscribe_in(
            &sidebar,
            window,
            move |this: &mut HomeView, _child, event: &ActiveRoomChange, _, cx| {
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

                    let room_id = room.room_id().to_owned();

                    let active_server = this.state.active_server().borrow().clone();
                    if let Some(active_server) = active_server {
                        breadcrumbs
                            .last_space_ids
                            .insert(active_server.room_id().to_owned(), room_id);
                    } else {
                        breadcrumbs.last_dm_id = Some(room_id);
                        breadcrumbs.dms_last = true;
                    }

                    this.state.breadcrumbs = breadcrumbs.clone();
                    let client = this.state.client.clone();
                    this.tokio_rt
                        .spawn(async move { set_account_data(&client, breadcrumbs).await });
                }

                this.update_chat_sidebar(cx);
            },
        )
        .detach();

        // Subscribe to SendMessage events to send them
        cx.subscribe_in(
            &chat_input,
            window,
            |this: &mut HomeView, _child, event: &SendMessageEvent, _window, _cx| {
                let text = event.text.clone();
                let active_room = this.active_room.borrow().clone();

                if let Some(room) = active_room {
                    let client = this.state.client.clone();
                    let timeline = this.state.timeline_manager.clone();
                    let room_id = room.room_id().to_owned();

                    this.tokio_rt.spawn(async move {
                        match send_message(text, client, timeline, room_id, None).await {
                            Ok(()) => tracing::debug!("Successfully sent message"),
                            Err(e) => tracing::error!("Did not send message: {e}"),
                        }
                    });
                }
            },
        )
        .detach();

        Self {
            server_list,
            tokio_rt,
            header,
            sidebar,
            chat,
            chat_input,

            chat_sidebar: ChatSidebar::from_active_room(
                state.active_room().borrow().clone(),
                &own_id,
                state.membership_map().borrow().clone(),
            ),

            state,
            active_room,
            membership_map,
            own_id,

            focus: focus_handle,
            overlay: Overlay::None,

            vim_mode: true,
        }
    }

    fn update_chat_sidebar(&mut self, cx: &mut Context<Self>) {
        let chat_sidebar = ChatSidebar::from_active_room(
            self.active_room.borrow().clone(),
            &self.own_id,
            self.membership_map.borrow().clone(),
        );

        if chat_sidebar != self.chat_sidebar {
            self.chat_sidebar = chat_sidebar;
            cx.notify();
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
        let structure = cx.structure();
        let key_context = KeyContext::parse(if self.vim_mode { "Home Vim" } else { "Home" })
            .expect("Could not parse key context");

        div()
            .track_focus(&self.focus)
            .id("home-view")
            .key_context(key_context)
            // Global Home View Actions
            .on_action(cx.listener(|this, _: &ToggleVimMode, _window, cx| {
                this.vim_mode = !this.vim_mode;
                tracing::debug!("Toggled vim mode: {}", this.vim_mode);
                cx.notify();
            }))
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
            // Focus & Chat Navigation Actions attached directly to root focus
            .on_action(cx.listener(|this, _: &FocusNext, window, cx| {
                tracing::debug!("Focusing next message");
                this.chat.update(cx, |that, cx| {
                    let mut new_focus = match that.focused_message {
                        Some(focus) => focus + 1,
                        None if !that.messages.is_empty() => that.messages.len() - 1,
                        None => return,
                    };

                    while let Some(item) = that.messages.get(new_focus) {
                        if item.is_user_message() {
                            that.focused_message = Some(new_focus);
                            that.list_state.set_follow_mode(FollowMode::Normal);
                            that.list_state.scroll_to_reveal_item(new_focus);
                            cx.notify();
                            window.focus(&that.focus_handle(cx), cx);
                            return;
                        }
                        new_focus += 1;
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &FocusPrevious, window, cx| {
                tracing::debug!("Focusing previous message");
                this.chat.update(cx, |that, cx| {
                    let mut new_focus = match that.focused_message {
                        Some(focus) => focus.saturating_sub(1),
                        None if !that.messages.is_empty() => that.messages.len().saturating_sub(1),
                        None => return,
                    };

                    while let Some(item) = that.messages.get(new_focus) {
                        if item.is_user_message() {
                            that.focused_message = Some(new_focus);
                            that.list_state.set_follow_mode(FollowMode::Normal);
                            that.list_state.scroll_to_reveal_item(new_focus);
                            cx.notify();
                            window.focus(&that.focus_handle(cx), cx);
                            return;
                        }
                        if new_focus == 0 {
                            return;
                        }
                        new_focus -= 1;
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &FocusInput, window, cx| {
                tracing::debug!("Focusing chat input");
                window.focus(&this.chat_input.focus_handle(cx), cx);
            }))
            .on_action(cx.listener(|this, action: &FocusInputWithKey, window, cx| {
                tracing::debug!("Focusing chat input with key");
                this.chat_input.update(cx, |bar, cx| {
                    bar.chat_input.update(cx, |input, cx| {
                        input.insert(&action.key, window, cx);
                    });
                });
                window.focus(&this.chat_input.focus_handle(cx), cx);
            }))
            .on_action(cx.listener(|this, _: &UnfocusInput, window, cx| {
                tracing::debug!("Restoring focus to Chat or Home View");
                window.focus(&this.focus, cx);
            }))
            .flex()
            .flex_row()
            .paddings(structure.gap)
            .gap(structure.gap)
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(structure.gap)
                    .child(
                        floating_tile(theme, structure)
                            .w(structure.server_column_width())
                            .child(
                                self.server_list.clone().cached(
                                    StyleRefinement::default()
                                        .w(structure.server_column_width())
                                        .h_full(),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(structure.sidebar.width)
                            .gap(structure.gap)
                            .child(
                                floating_tile(theme, structure).flex_grow_1().child(
                                    self.sidebar
                                        .clone()
                                        .cached(StyleRefinement::default().size_full()),
                                ),
                            )
                            .child(floating_tile(theme, structure).h(structure.header.height)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(structure.gap)
                    .size_full()
                    .child(
                        floating_tile(theme, structure)
                            .h(structure.header.height)
                            .child(self.header.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(structure.gap)
                            .size_full()
                            .child(
                                floating_tile(theme, structure)
                                    .flex_grow_1()
                                    .h_full()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .size_full()
                                            .paddings(structure.gap)
                                            .pt_0()
                                            .child(
                                                self.chat
                                                    .clone()
                                                    .cached(StyleRefinement::default().size_full()),
                                            )
                                            .child(self.chat_input.clone()),
                                    ),
                            )
                            .when_some(self.chat_sidebar.clone(), |el, sidebar| {
                                el.child(
                                    floating_tile(theme, structure)
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
