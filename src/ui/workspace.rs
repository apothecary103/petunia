use gpui::prelude::*;
use gpui::{App, Context, Entity, Subscription, Window, div};
use gpui_component::{ActiveTheme, StyledExt};

use super::linking::Linking;
use crate::store::{Store, StoreEvent};

/// The root view. Shows the linking screen until an account exists, then the
/// conversation shell.
pub struct Workspace {
    store: Entity<Store>,
    screen: Screen,
    _subscriptions: Vec<Subscription>,
}

enum Screen {
    Linking(Entity<Linking>),
    Main,
}

impl Workspace {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let linking = cx.new(|cx| Linking::new(store.clone(), cx));

        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe_in(&store, window, Self::on_store_event),
        ];

        Self {
            store,
            screen: Screen::Linking(linking),
            _subscriptions: subscriptions,
        }
    }

    fn on_store_event(
        &mut self,
        _store: &Entity<Store>,
        event: &StoreEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let StoreEvent::Linked = event {
            self.screen = Screen::Main;
            cx.notify();
        }
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(match &self.screen {
                Screen::Linking(linking) => linking.clone().into_any_element(),
                Screen::Main => div()
                    .size_full()
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.muted_foreground)
                    .child("Linked. The conversation shell lands in the next phase.")
                    .into_any_element(),
            })
    }
}
