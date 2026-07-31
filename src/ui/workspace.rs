use gpui::prelude::*;
use gpui::{App, Context, Entity, SharedString, Subscription, Window, div, px};
use gpui_component::{ActiveTheme, IconName};

use super::conversation::Conversation;
use super::details::Details;
use super::kit;
use super::linking::Linking;
use super::palette::{Dismissed, Switcher};
use super::sidebar::Sidebar;
use crate::actions;
use crate::session::Session;
use crate::store::{Store, StoreEvent};
use crate::theme::ActivePalette;

/// Tall enough to clear the traffic lights, which macOS draws at a fixed size.
pub const TITLE_BAR: f32 = 40.0;

/// The root view. Shows the linking screen until an account exists, then the
/// conversation shell.
pub struct Workspace {
    store: Entity<Store>,
    /// Actions dispatch along the focus path, so the root has to be in it or
    /// nothing bound to a key ever reaches this view.
    focus: gpui::FocusHandle,
    screen: Screen,
    session: Session,
    /// Present only while it is up, so nothing renders or listens otherwise.
    switcher: Option<Entity<Switcher>>,
    _subscriptions: Vec<Subscription>,
}

enum Screen {
    Linking(Entity<Linking>),
    Main {
        sidebar: Entity<Sidebar>,
        conversation: Entity<Conversation>,
        details: Entity<Details>,
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
            focus: cx.focus_handle(),
            screen: Screen::Linking(linking),
            session: Session::load(),
            switcher: None,
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
        let details = cx.new(|cx| Details::new(self.store.clone(), cx));

        if let Some(thread) = self.session.active.clone() {
            self.store
                .update(cx, |store, cx| store.activate(thread, cx));
        }

        self.screen = Screen::Main {
            sidebar,
            conversation,
            details,
        };
    }

    fn on_store_event(
        &mut self,
        _store: &Entity<Store>,
        event: &StoreEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            StoreEvent::Linked => {
                self.enter_main(cx);
                cx.notify();
            }
            // Clicking a name is a request to see it, so the panel opens
            // itself rather than making you find the toggle.
            StoreEvent::Inspecting => {
                if !self.session.details.open {
                    self.session.details.open = true;
                    self.session.save();
                }
                cx.notify();
            }
            _ => {}
        }
    }

    /// cmd+k. Raising it again while it is up refocuses and clears it, which is
    /// what pressing the shortcut twice is asking for.
    fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Main { .. }) {
            return;
        }

        let switcher = self.switcher.get_or_insert_with(|| {
            let switcher = cx.new(|cx| Switcher::new(self.store.clone(), cx));
            cx.subscribe(&switcher, |this, _, _: &Dismissed, cx| {
                this.switcher = None;
                cx.notify();
            })
            .detach();
            switcher
        });

        switcher.update(cx, |switcher, cx| switcher.reset(window, cx));
        cx.notify();
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

impl gpui::Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus.clone()
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
                details,
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

                // The sidebar runs the full height of the window and the
                // traffic lights float over its own top padding, so the header
                // belongs to the conversation column rather than spanning
                // everything. A full-width strip left a dead band above the
                // sidebar, which is what made the titlebar look wrong.
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
                            .flex()
                            .flex_col()
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
                                .child(details.clone()),
                        )
                    })
                    .into_any_element()
            }
        };

        div()
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .on_action(cx.listener(|this, _: &actions::ToggleSidebar, _, cx| {
                this.toggle_sidebar(cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ToggleDetails, _, cx| {
                this.toggle_details(cx)
            }))
            .on_action(cx.listener(|this, _: &actions::QuickSwitcher, window, cx| {
                this.open_switcher(window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::Cancel, _, cx| {
                if this.switcher.take().is_some() {
                    cx.notify();
                }
            }))
            .child(body)
            .children(self.switcher.clone())
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
        // No border underneath: the reference separates the columns, not the
        // strip, and a rule here cuts the window in half.
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            // Clears the traffic lights when the sidebar is not there to.
            .when(!self.session.sidebar.open, |this| this.pl(px(84.0)))
            .h(px(TITLE_BAR))
            .flex_none()
            .child(kit::icon_button(
                "toggle-sidebar",
                if self.session.sidebar.open {
                    IconName::PanelLeftClose
                } else {
                    IconName::PanelLeftOpen
                },
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
            .child(kit::icon_button(
                "toggle-details",
                if self.session.details.open {
                    IconName::PanelRightClose
                } else {
                    IconName::PanelRightOpen
                },
                palette,
                cx.listener(|this, _, _, cx| this.toggle_details(cx)),
            ))
    }
}

