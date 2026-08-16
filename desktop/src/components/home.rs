use std::sync::Arc;

use deplace_core::{
    get_other_member,
    matrix_api::account_data::set_account_data,
    state::{AppState, MembershipMap},
};
use gpui::{
    AppContext, Context, Entity, FocusHandle, Focusable, FollowMode, InteractiveElement,
    IntoElement, KeyContext, ParentElement, Pixels, Render, StyleRefinement, Styled, Subscription,
    Window, actions, div, prelude::FluentBuilder,
};
use gpui_component::StyledExt;
use matrix_sdk::{
    Room,
    room::RoomMember,
    ruma::{OwnedUserId, UserId},
};
use tokio::sync::watch::Receiver;

use crate::{
    components::{
        AvatarCache,
        cache::ThumbnailCache,
        chat::{
            ChatView, FocusInput, FocusInputWithKey, FocusNext, FocusPrevious, UnfocusInput,
            input::{ChatInputBar, SendEvent},
        },
        floating_tile,
        header::HeaderView,
        overlay::{Close, Overlay},
        quick_select,
        server_list::ServerListView,
        sidebar::SidebarView,
    },
    room_state::RoomStateStore,
    theme::{DeplaceThings, Structure},
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

    room_store: RoomStateStore,

    active_room: Receiver<Option<Room>>,
    membership_map: Receiver<MembershipMap>,
    own_id: OwnedUserId,
    avatar_cache: AvatarCache,

    overlay: Entity<Overlay>,
    overlay_subscription: Option<Subscription>, // keep subscription unique instead of detaching

    vim_mode: bool,
}

actions!(home, [ToggleVimMode]);

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

impl HomeView {
    pub fn new(
        tokio_rt: Arc<tokio::runtime::Runtime>,
        state: AppState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let avatar_cache: AvatarCache = AvatarCache::new(state.client(), tokio_rt.clone());
        let image_cache: ThumbnailCache = ThumbnailCache::new(state.client(), tokio_rt.clone());

        let room_store = RoomStateStore::new();

        let server_list = cx.new(|cx| {
            ServerListView::new(state.clone(), cx, tokio_rt.clone(), avatar_cache.clone())
        });
        let header =
            cx.new(|cx| HeaderView::new(state.clone(), cx, tokio_rt.clone(), avatar_cache.clone()));
        let sidebar = cx
            .new(|cx| SidebarView::new(state.clone(), cx, tokio_rt.clone(), avatar_cache.clone()));
        let chat = cx.new(|cx| {
            ChatView::new(
                &state,
                cx,
                tokio_rt.clone(),
                avatar_cache.clone(),
                image_cache.clone(),
                room_store.clone(),
            )
        });
        let chat_input = cx.new(|cx| ChatInputBar::new(&state, window, cx, room_store.clone()));

        let active_room = state.active_room();
        let membership_map = state.membership_map();
        let own_id = state.user_device().user_id.clone();

        notify_on_change(active_room.clone(), cx);
        notify_on_change(membership_map.clone(), cx);

        let overlay = cx.new(|cx| Overlay::new(window, cx));

        cx.subscribe_in(
            &server_list,
            window,
            move |this: &mut HomeView,
                  _child,
                  event: &ActiveServerChange,
                  &mut _,
                  cx: &mut Context<Self>| {
                let server = event.room();

                this.state.set_active_server(server.clone());

                this.update_chat_sidebar(cx);
            },
        )
        .detach();

        cx.subscribe_in(
            &sidebar,
            window,
            move |this: &mut HomeView, _, event: &ActiveRoomChange, _, cx| {
                let room = event.room();
                this.state.set_active_room(room.clone());
                this.update_chat_sidebar(cx);
            },
        )
        .detach();

        // Subscribe to SendMessage events to send them
        cx.subscribe_in(
            &chat_input,
            window,
            |this: &mut HomeView, _, event: &SendEvent, _, _| {
                let Some(room) = this.active_room.borrow().clone() else {
                    tracing::warn!("Tried to send event but no active room");
                    return;
                };

                let timeline_manager = this.state.timeline_manager();

                match event.clone() {
                    SendEvent::SendMessage {
                        text,
                        attachments,
                        in_reply_to,
                    } => {
                        let attachments_empty = attachments.is_empty();

                        if !attachments_empty {
                            let room = room.clone();
                            let timeline_manager = timeline_manager.clone();
                            let in_reply_to = in_reply_to.clone();

                            this.tokio_rt.spawn(async move {
                                let mut iter = attachments.into_iter();

                                if let Some(attachment) = iter.next() {
                                    if let Err(e) = timeline_manager
                                        .send_attachment(&room, attachment, in_reply_to.clone())
                                        .await
                                    {
                                        tracing::error!("Failed to send attachment: {}", e);
                                    }
                                }

                                for attachment in iter {
                                    if let Err(e) = timeline_manager
                                        .send_attachment(&room, attachment, None)
                                        .await
                                    {
                                        tracing::error!("Failed to send attachment: {}", e);
                                    }
                                }
                            });
                        }

                        if !text.trim().is_empty() {
                            this.tokio_rt.spawn(async move {
                                if let Err(e) = timeline_manager
                                    .send_message(
                                        text,
                                        &room,
                                        attachments_empty.then(|| in_reply_to.clone()).flatten(),
                                    )
                                    .await
                                {
                                    tracing::error!("Failed to send message: {}", e);
                                }
                            });
                        }
                    }
                    _ => {}
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

            room_store,

            chat_sidebar: ChatSidebar::from_active_room(
                state.active_room().borrow().clone(),
                &own_id,
                state.membership_map().borrow().clone(),
            ),

            state,
            active_room,
            membership_map,
            own_id,
            avatar_cache,

            focus: focus_handle,
            overlay,
            overlay_subscription: None,

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

                    // Subscribe to Close events
                    this.overlay_subscription = Some(cx.subscribe_in(
                        &this.overlay,
                        window,
                        |this: &mut HomeView, _child, event: &Close, window, cx| {
                            tracing::debug!("Closing quick select overlay");
                            if let Some(room_id) = &event.room_id {
                                let room = this.state.client().get_room(&room_id);
                                this.state.set_active_room(room);
                            }
                            this.overlay
                                .update(cx, |that, cx| that.close_overlay(window, cx));
                            window.focus(&this.focus, cx);
                            cx.notify();
                        },
                    ));

                    this.overlay.update(cx, |overlay, cx| {
                        overlay.open_quick_select(
                            window,
                            cx,
                            this.state.clone(),
                            this.avatar_cache.clone(),
                        )
                    });
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
                                            .paddings(structure.small_gap)
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
            .child(self.overlay.clone())
    }
}
