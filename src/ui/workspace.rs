use gpui::prelude::*;
use gpui::{App, Context, Entity, MouseButton, SharedString, Subscription, Window, div, px};
use gpui_component::{ActiveTheme, StyledExt};

use super::conversation::Conversation;
use super::linking::Linking;
use super::sidebar::Sidebar;
use crate::actions;
use crate::session::Session;
use crate::store::{Store, StoreEvent};
use crate::theme::ActivePalette;

/// The root view. Shows the linking screen until an account exists, then the
/// conversation shell.
pub struct Workspace {
    store: Entity<Store>,
    screen: Screen,
    session: Session,
    _subscriptions: Vec<Subscription>,
}

enum Screen {
    Linking(Entity<Linking>),
    Main {
        sidebar: Entity<Sidebar>,
        conversation: Entity<Conversation>,
    },
}

impl Workspace {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let linking = cx.new(|cx| Linking::new(store.clone(), cx));

        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe_in(&store, window, Self::on_store_event),
        ];

        let mut workspace = Self {
            store: store.clone(),
            screen: Screen::Linking(linking),
            session: Session::load(),
            _subscriptions: subscriptions,
        };

        // An already-linked store never emits `Linked` a second time, so the
        // shell has to come up if the account is there on the first read.
        if store.read(cx).state().is_some() {
            workspace.enter_main(cx);
        }
        workspace
    }

    fn enter_main(&mut self, cx: &mut Context<Self>) {
        let sidebar = cx.new(|cx| Sidebar::new(self.store.clone(), cx));
        let conversation = cx.new(|cx| Conversation::new(self.store.clone(), cx));

        if let Some(thread) = self.session.active.clone() {
            self.store
                .update(cx, |store, cx| store.activate(thread, cx));
        }

        self.screen = Screen::Main {
            sidebar,
            conversation,
        };
    }

    fn on_store_event(
        &mut self,
        _store: &Entity<Store>,
        event: &StoreEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let StoreEvent::Linked = event {
            self.enter_main(cx);
            cx.notify();
        }
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.session.sidebar.open = !self.session.sidebar.open;
        self.session.save();
        cx.notify();
    }

    fn toggle_details(&mut self, cx: &mut Context<Self>) {
        self.session.details.open = !self.session.details.open;
        self.session.save();
        cx.notify();
    }

    /// The unread count belongs in the window title until there is somewhere
    /// better to put it.
    pub fn title(&self, cx: &App) -> String {
        let unread: u32 = self
            .store
            .read(cx)
            .state()
            .map(|state| state.index.total_unread())
            .unwrap_or(0);

        match unread {
            0 => "Petunia".into(),
            unread => format!("({unread}) Petunia"),
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let border = cx.theme().border;
        window.set_window_title(&self.title(cx));

        let body = match &self.screen {
            Screen::Linking(linking) => linking.clone().into_any_element(),
            Screen::Main {
                sidebar,
                conversation,
            } => {
                let title = self
                    .store
                    .read(cx)
                    .active()
                    .and_then(|thread| {
                        self.store
                            .read(cx)
                            .state()
                            .map(|state| state.title(thread))
                    })
                    .unwrap_or_default();

                div()
                    .size_full()
                    .flex()
                    .when(self.session.sidebar.open, |this| {
                        this.child(
                            div()
                                .w(px(self.session.sidebar.width))
                                .flex_none()
                                .border_r_1()
                                .border_color(border)
                                .child(sidebar.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .v_flex()
                            .child(self.header(&title, &palette, cx))
                            .child(div().flex_1().min_h_0().child(conversation.clone())),
                    )
                    .when(self.session.details.open, |this| {
                        this.child(
                            div()
                                .w(px(self.session.details.width))
                                .flex_none()
                                .border_l_1()
                                .border_color(border)
                                .bg(palette.surface)
                                .p_4()
                                .text_color(palette.text_muted)
                                .child("Details"),
                        )
                    })
                    .into_any_element()
            }
        };

        div()
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .on_action(cx.listener(|this, _: &actions::ToggleSidebar, _, cx| {
                this.toggle_sidebar(cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ToggleDetails, _, cx| {
                this.toggle_details(cx)
            }))
            .child(body)
    }
}

impl Workspace {
    /// A thin strip carrying the conversation's name and the panel toggles, in
    /// place of the reference's tab bar.
    fn header(
        &self,
        title: &str,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .h(px(38.0))
            .flex_none()
            .border_b_1()
            .border_color(palette.border)
            .child(toggle(
                "toggle-sidebar",
                "▚",
                palette,
                cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text)
                    .child(SharedString::from(title.to_owned())),
            )
            .child(toggle(
                "toggle-details",
                "▐",
                palette,
                cx.listener(|this, _, _, cx| this.toggle_details(cx)),
            ))
    }
}

fn toggle(
    id: &'static str,
    glyph: &'static str,
    palette: &crate::config::Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .text_size(px(11.0))
        .text_color(palette.text_muted)
        .hover(|this| this.bg(palette.hover))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(glyph)
}
