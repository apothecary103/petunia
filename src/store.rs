use std::sync::Arc;

use gpui::{Context, EventEmitter};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, warn};
use uuid::Uuid;

use crate::config::Config;
use crate::data::State;
use crate::signal::{self, Command, Connection, Event};

/// Everything the views read, and the one way they talk back to the Signal
/// worker. Views observe this entity rather than owning any of it.
pub struct Store {
    pub config: Arc<Config>,
    /// `None` until the worker reports which account it linked.
    state: Option<State>,
    commands: Option<UnboundedSender<Command>>,
    /// The linking QR, while there is one to show.
    pub link_url: Option<String>,
    pub link_failure: Option<String>,
}

/// What views react to. A repaint alone is `cx.notify()`; these are the moments
/// that need more than a repaint.
#[derive(Debug, Clone)]
pub enum StoreEvent {
    Linked,
    /// A page of history arrived for a thread, and the list has to keep its
    /// scroll position if it was a page of *older* messages.
    History { older: bool },
    Failed(String),
}

impl EventEmitter<StoreEvent> for Store {}

impl Store {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            state: None,
            commands: None,
            link_url: None,
            link_failure: None,
        }
    }

    pub fn state(&self) -> Option<&State> {
        self.state.as_ref()
    }

    pub fn connection(&self) -> Connection {
        self.state
            .as_ref()
            .map(|state| state.connection)
            .unwrap_or_default()
    }

    pub fn send(&self, command: Command) {
        match &self.commands {
            Some(sender) => {
                let _ = sender.send(command);
            }
            None => warn!("signal worker not ready, dropping command"),
        }
    }

    pub fn config_changed(&mut self, config: Arc<Config>, cx: &mut Context<Self>) {
        self.config = config;
        cx.notify();
    }

    /// Routes one worker event into the model. The only place `State` is
    /// mutated from outside a view.
    pub fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Ready(sender) => {
                self.commands = Some(sender);
                return;
            }
            Event::LinkUrl(url) => {
                self.link_url = Some(url);
            }
            Event::Linked { aci } => {
                self.link_url = None;
                self.link_failure = None;
                self.state = Some(State::new(aci));
                cx.emit(StoreEvent::Linked);
            }
            Event::Error(message) if self.state.is_none() => {
                error!(%message, "signal error while linking");
                self.link_failure = Some(message);
            }
            event => self.apply_to_state(event, cx),
        }
        cx.notify();
    }

    fn apply_to_state(&mut self, event: Event, cx: &mut Context<Self>) {
        let Some(state) = self.state.as_mut() else {
            warn!(?event, "event arrived before linking; dropped");
            return;
        };

        match event {
            Event::Contacts { contacts, groups } => state.contacts_updated(contacts, groups),
            Event::Profile { uuid, name } => state.set_profile(uuid, name),
            Event::Avatar { thread, path } => {
                state.avatars.insert(thread, path);
            }
            Event::Typing {
                thread,
                sender,
                started,
            } => state.set_typing(&thread, sender, started),
            Event::Connection(connection) => state.connection = connection,
            Event::History {
                thread,
                messages,
                more,
                older,
            } => {
                let history = state.history_mut(&thread);
                if older {
                    history.prepend(messages, more);
                } else {
                    history.merge(messages, more);
                }
                cx.emit(StoreEvent::History { older });
            }
            Event::Error(message) => {
                error!(%message, "signal error");
                cx.emit(StoreEvent::Failed(message));
            }
            // Fragments, attachments, previews and receipts land in Phase 3 and
            // 4 alongside the views that render them.
            other => tracing::debug!(?other, "event not routed yet"),
        }
    }

    /// Our own account id, once linked.
    pub fn aci(&self) -> Option<Uuid> {
        self.state.as_ref().map(|state| state.aci)
    }
}

/// Spawns the Signal worker and forwards its events into the store.
pub fn bridge(store: gpui::Entity<Store>, cx: &mut gpui::App) {
    signal::bridge::spawn(store, cx);
}
