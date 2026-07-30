use std::sync::Arc;

use deplace_core::state::AppState;
use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Pixels, Render, Styled, Window, div,
    prelude::FluentBuilder,
};
use gpui_component::StyledExt;
use matrix_sdk::ruma::OwnedUserId;

use crate::{
    components::{dm_list::DmListView, floating_tile, server_list::ServerListView},
    theme::{ActiveAppTheme, Structure},
};

pub struct HomeView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    state: AppState,
    chat_sidebar: Option<ChatSidebar>,
    server_list: Entity<ServerListView>,
    dm_list: Entity<DmListView>,
}

#[derive(Clone)]
enum ChatSidebar {
    Members,
    Search,
    Pinned,
    Member(OwnedUserId),
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
        cx: &mut Context<Self>,
    ) -> Self {
        let server_list = cx.new(|cx| ServerListView::new(&state, cx, tokio_rt.clone()));

        let dm_list = cx.new(|cx| DmListView::new(&state, cx));

        Self {
            server_list,
            dm_list,
            tokio_rt,
            state,
            chat_sidebar: Some(ChatSidebar::Members),
        }
    }
}

impl Render for HomeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let padding = theme.tile.padding;
        let structure = &theme.structure;

        div()
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
                            .w(structure.server_column.width)
                            .child(self.server_list.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(structure.sidebar_width)
                            .gap(padding)
                            .child(floating_tile(theme).flex_grow_1())
                            .child(floating_tile(theme).h(structure.header_height)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(padding)
                    .size_full()
                    .child(floating_tile(theme).h(structure.header_height))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(padding)
                            .size_full()
                            .child(floating_tile(theme).flex_grow_1().h_full())
                            .when_some(self.chat_sidebar.clone(), |el, sidebar| {
                                el.child(
                                    floating_tile(theme)
                                        .w(sidebar.get_width(structure))
                                        .h_full(),
                                )
                            }),
                    ),
            )
    }
}
