use std::sync::Arc;

use crate::{
    cache::VideoCache,
    components::chat::{FocusNext, FocusPrevious, ReplyToMessage},
};

use deplace_core::{
    APP_HUMAN_NAME, NameExt, get_other_member,
    helpers::RoomPlaceholderExt,
    state::{AppState, MembershipMap},
};
use gpui::{
    AppContext, Context, Element, Entity, FocusHandle, Focusable, FollowMode, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, StyleRefinement, Styled, Subscription, Window,
    actions, div, prelude::FluentBuilder,
};
use gpui_component::StyledExt;
use macros::{nonzero_usize, tailwind_div};
use matrix_sdk::{
    Room,
    room::RoomMember,
    ruma::{OwnedUserId, UserId},
};
use tokio::sync::watch::Receiver;

use crate::{
    cache::{AvatarCache, ThumbnailCache},
    components::{
        chat::{
            ChatTimelineCache, ChatView, EditMessage, FocusInput, FocusInputWithKey, ScrollOffset,
            UnfocusInput,
        },
        floating_tile,
        header::HeaderView,
        overlay::{Close, Open, Overlay},
        server_list::ServerListView,
        sidebar::SidebarView,
    },
    things::{DeplaceThings, Structure},
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

    chat: Entity<ChatTimelineCache>,

    active_room: Receiver<Option<Room>>,
    membership_map: Receiver<MembershipMap>,
    own_id: OwnedUserId,

    avatar_cache: AvatarCache,
    image_cache: ThumbnailCache,
    video_cache: VideoCache,

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
        let avatar_cache = AvatarCache::new(state.client(), tokio_rt.clone());
        let image_cache = ThumbnailCache::new(state.client(), tokio_rt.clone());
        let video_cache = VideoCache::new(state.client(), tokio_rt.clone());

        let server_list = cx.new(|cx| {
            ServerListView::new(state.clone(), cx, tokio_rt.clone(), avatar_cache.clone())
        });
        let header =
            cx.new(|cx| HeaderView::new(state.clone(), cx, tokio_rt.clone(), avatar_cache.clone()));
        let sidebar = cx
            .new(|cx| SidebarView::new(state.clone(), cx, tokio_rt.clone(), avatar_cache.clone()));

        let chat = cx.new(|_| {
            ChatTimelineCache::new(nonzero_usize!(15), || tailwind_div!(size_full).into_any())
        });

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
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                let server = event.room();

                this.state.set_active_server(server.clone());

                this.update_chat_sidebar(cx);

                let room = this.state.active_room().borrow().clone();
                this.load_room_chat(room.clone(), cx, window);
                this.set_active_room_title(window);
            },
        )
        .detach();

        cx.subscribe_in(
            &sidebar,
            window,
            move |this: &mut HomeView, _, event: &ActiveRoomChange, window, cx| {
                let room = event.room();
                this.state.set_active_room(room.clone());
                this.update_chat_sidebar(cx);
                this.load_room_chat(room.clone(), cx, window);
                this.set_active_room_title(window);
            },
        )
        .detach();

        let mut view = Self {
            server_list,
            tokio_rt,
            header,
            sidebar,

            chat_sidebar: ChatSidebar::from_active_room(
                state.active_room().borrow().clone(),
                &own_id,
                state.membership_map().borrow().clone(),
            ),

            chat,

            state: state.clone(),
            active_room,
            membership_map,
            own_id,

            avatar_cache,
            image_cache,
            video_cache,

            focus: focus_handle,
            overlay,
            overlay_subscription: None,

            vim_mode: true,
        };

        view.load_room_chat(state.active_room().borrow().clone(), cx, window);
        view.set_active_room_title(window);

        view
    }

    fn set_active_room_title(&mut self, window: &mut Window) {
        let room_name = self
            .state
            .active_room()
            .borrow()
            .clone()
            .map(|r| r.get_input_placeholder());

        let server_name = self
            .state
            .active_server()
            .borrow()
            .clone()
            .map(|r| r.get_name());

        let text = match (room_name, server_name) {
            (Some(room_name), Some(server_name)) => format!("{} | {}", room_name, server_name),
            (Some(room_name), None) => format!("{} - {APP_HUMAN_NAME}", room_name),
            (None, Some(server_name)) => format!("{} - {APP_HUMAN_NAME}", server_name),
            (None, None) => APP_HUMAN_NAME.to_string(),
        };
        window.set_window_title(&text);
    }

    fn load_room_chat(&mut self, room: Option<Room>, cx: &mut Context<Self>, window: &mut Window) {
        self.chat.update(cx, |that, cx| {
            if let Some(room) = room {
                let room_id = room.room_id().to_owned();

                if !that.show(&room_id, window, cx) {
                    that.insert(
                        room_id,
                        cx.new(|cx| {
                            ChatView::new(
                                &self.state,
                                cx,
                                window,
                                self.tokio_rt.clone(),
                                self.avatar_cache.clone(),
                                self.image_cache.clone(),
                                self.video_cache.clone(),
                                room,
                            )
                        }),
                        window,
                        cx,
                    );
                }
            } else {
                that.hide_all(cx);
            }
        })
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

        div()
            .track_focus(&self.focus)
            .id("home-view")
            .key_context(if self.vim_mode { "Home Vim" } else { "Home" })
            // Global Home View Actions
            .on_action(cx.listener(|this, _: &ToggleVimMode, _window, cx| {
                this.vim_mode = !this.vim_mode;
                tracing::debug!("Toggled vim mode: {}", this.vim_mode);
                cx.notify();
            }))
            .on_action(cx.listener(|this, action: &Open, window, cx| {
                tracing::debug!(
                    "Opening {}",
                    match action {
                        Open::Settings => "settings",
                        Open::QuickSelect => "quick select",
                    }
                );

                // Subscribe to Close events
                this.overlay_subscription = Some(cx.subscribe_in(
                    &this.overlay,
                    window,
                    |this: &mut HomeView, _child, event: &Close, window, cx| {
                        tracing::debug!("Closing quick select overlay");
                        if let Close::QuickSelect(Some(room_id)) = &event {
                            let room = this.state.client().get_room(room_id);
                            this.state.set_active_room(room.clone());
                            this.load_room_chat(room, cx, window);
                        } else {
                            window.focus(&this.focus, cx);
                        }
                        this.overlay
                            .update(cx, |that, cx| that.close_overlay(window, cx));
                        cx.notify();
                    },
                ));

                this.overlay.update(cx, |overlay, cx| match action {
                    Open::Settings => {
                        overlay.open_settings(window, cx, this.state.clone(), this.tokio_rt.clone())
                    }
                    Open::QuickSelect => overlay.open_quick_select(
                        window,
                        cx,
                        this.state.clone(),
                        this.avatar_cache.clone(),
                    ),
                });
                cx.notify();
            }))
            // Focus & Chat Navigation Actions attached directly to root focus
            .on_action(cx.listener(|this, _: &ReplyToMessage, window, cx| {
                tracing::debug!("Replying to Message");
                this.chat.update(cx, |that, cx| {
                    that.update_visible_chat(cx, |chat, cx| {
                        let timeline = chat.read_timeline(cx);
                        let Some(message) = timeline.get_focused_item() else {
                            return;
                        };
                        let replying_to = match message.try_into() {
                            Ok(replying_to) => replying_to,
                            Err(e) => {
                                tracing::debug!("Can't reply to message: {}", e);
                                return;
                            }
                        };
                        chat.update_timeline(cx, |timeline, _| {
                            let Some(_) = timeline.try_reply_to_message() else {
                                tracing::debug!("Can't reply to message");
                                return;
                            };
                        });
                        chat.update_input(cx, |input, _| {
                            input.reply_to(replying_to);
                        });
                        window.focus(&chat.focus_handle(cx), cx);
                        cx.notify();
                    });
                })
            }))
            .on_action(cx.listener(|this, _: &EditMessage, window, cx| {
                tracing::debug!("Editing focused message");
                this.chat.update(cx, |that, cx| {
                    that.update_visible_timeline(cx, |timeline, cx| {
                        if let Some((idx, _, _)) = timeline.focused_message {
                            if timeline.try_edit_message(window, cx, idx).is_none() {
                                tracing::debug!("Failed to start editing message")
                            } else {
                                cx.notify();
                            };
                        }
                    });
                });
            }))
            .on_action(cx.listener(|this, _: &FocusNext, window, cx| {
                tracing::trace!("Focusing next message");
                this.chat.update(cx, |that, cx| {
                    that.update_visible_timeline(cx, |timeline, cx| {
                        let mut new_focus = match timeline.focused_message {
                            Some(focus) => focus.0 + 1,
                            None if !timeline.messages.is_empty() => timeline.messages.len() - 1,
                            None => return,
                        };

                        while let Some(item) = timeline.messages.get(new_focus) {
                            if item.is_user_message() {
                                timeline.focused_message = Some((new_focus, false, false));
                                timeline.list_state.set_follow_mode(FollowMode::Normal);
                                timeline.list_state.scroll_to_reveal_item(new_focus);
                                cx.notify();
                                window.focus(&timeline.focus_handle(cx), cx);
                                return;
                            }
                            new_focus += 1;
                        }
                    });
                });
            }))
            .on_action(cx.listener(|this, _: &FocusPrevious, window, cx| {
                tracing::trace!("Focusing previous message");
                this.chat.update(cx, |that, cx| {
                    that.update_visible_timeline(cx, |timeline, cx| {
                        let mut new_focus = match timeline.focused_message {
                            Some((focus, _, _)) => focus.saturating_sub(1),
                            None if !timeline.messages.is_empty() => {
                                timeline.messages.len().saturating_sub(1)
                            }
                            None => return,
                        };

                        while let Some(item) = timeline.messages.get(new_focus) {
                            if item.is_user_message() {
                                timeline.focused_message = Some((new_focus, false, false));
                                timeline.list_state.set_follow_mode(FollowMode::Normal);
                                timeline.list_state.scroll_to_reveal_item(new_focus);
                                cx.notify();
                                window.focus(&timeline.focus_handle(cx), cx);
                                timeline.check_pagination(
                                    ScrollOffset::FocusedIndex { index: new_focus },
                                    cx,
                                );
                                return;
                            }
                            if new_focus == 0 {
                                return;
                            }
                            new_focus -= 1;
                        }
                    })
                });
            }))
            .on_action(cx.listener(|this, _: &FocusInput, window, cx| {
                tracing::debug!("Focusing chat input");
                if let Some(chat_view) = this.chat.read(cx).visible().cloned() {
                    window.focus(&chat_view.focus_handle(cx), cx);
                }
            }))
            .on_action(cx.listener(|this, action: &FocusInputWithKey, window, cx| {
                tracing::debug!("Focusing chat input with key");
                if let Some(chat_view) = this.chat.read(cx).visible().cloned() {
                    chat_view.update(cx, |view, cx| view.insert_at_input(&action.key, window, cx));
                    window.focus(&chat_view.focus_handle(cx), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &UnfocusInput, window, cx| {
                this.chat.update(cx, |that, cx| {
                    that.update_visible_chat_input(cx, |input, cx| {
                        if input.remove_reply() {
                            tracing::debug!(
                                "Removing reply from input instead of unfocusing input"
                            );
                            cx.notify();
                        } else {
                            tracing::debug!("Restoring focus to Chat or Home View");
                            window.focus(&this.focus, cx);
                        }
                    });
                    that.update_visible_timeline(cx, |timeline, cx| {
                        if let Some((_, _, replying)) = &mut timeline.focused_message {
                            *replying = false
                        }
                    })
                });
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
                                self.chat
                                    .clone()
                                    .cached(StyleRefinement::default().size_full()),
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
