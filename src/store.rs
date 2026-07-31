use std::sync::Arc;

use gpui::{Context, EventEmitter};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, warn};

use crate::config::Config;
use crate::data::{self, Fragment, History, State, Thread};
use crate::signal::{Command, Connection, Event};

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
    /// The conversation on screen. A message arriving here is read, not unread.
    active: Option<Thread>,
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
            active: None,
        }
    }

    pub fn state(&self) -> Option<&State> {
        self.state.as_ref()
    }

    pub fn active(&self) -> Option<&Thread> {
        self.active.as_ref()
    }

    /// Opens a conversation, loading its newest page the first time.
    pub fn activate(&mut self, thread: Thread, cx: &mut Context<Self>) {
        if self.active.as_ref() == Some(&thread) {
            return;
        }

        let unseen = self
            .state
            .as_ref()
            .is_none_or(|state| state.history(&thread).is_none());
        if unseen {
            self.send(Command::load(thread.clone()));
        }
        if let Some(state) = self.state.as_mut() {
            state.index.clear_unread(&thread);
        }

        self.active = Some(thread);
        cx.notify();
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
            Event::Attachment { thread, id, blob } => {
                state.history_mut(&thread).set_blob(&id, blob);
            }
            Event::Preview { thread, message } => state.record(&thread, &message),
            Event::History {
                thread,
                messages,
                more,
                older,
            } => {
                if !older
                    && let Some(last) = messages.last().cloned()
                {
                    state.record(&thread, &last);
                }
                let history = state.history_mut(&thread);
                if older {
                    history.prepend(messages, more);
                } else {
                    history.merge(messages, more);
                }
                cx.emit(StoreEvent::History { older });
            }
            Event::MessageStatus { timestamps, status } => {
                let aci = state.aci;
                for history in state.histories.values_mut() {
                    history.apply_status(&timestamps, aci, status);
                }
            }
            Event::Fragment {
                thread,
                fragment,
                order,
            } => self.fragment(thread, fragment, order),
            Event::Error(message) => {
                error!(%message, "signal error");
                cx.emit(StoreEvent::Failed(message));
            }
            Event::Ready(_) | Event::LinkUrl(_) | Event::Linked { .. } => {}
        }
    }

    fn fragment(&mut self, thread: Thread, fragment: Fragment, order: u64) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match fragment {
            Fragment::Message(message) => self.message(thread, message),
            Fragment::Edit { target, message } => {
                state.history_mut(&thread).apply_edit(&target, message, order);
                self.refresh_preview(&thread);
            }
            Fragment::Reaction {
                target,
                reaction,
                remove,
            } => {
                state
                    .history_mut(&thread)
                    .apply_reaction(&target, reaction, remove);
            }
            Fragment::Delete { target } => {
                state.history_mut(&thread).apply_delete(&target);
                self.refresh_preview(&thread);
            }
            Fragment::Ignored => {}
        }
    }

    fn message(&mut self, thread: Thread, message: data::Message) {
        let active = self.active.clone();
        let Some(state) = self.state.as_mut() else {
            return;
        };

        state.record(&thread, &message);

        if message.sender() != state.aci && active.as_ref() != Some(&thread) {
            let mentioned = message.mentions(state.aci);
            state.index.mark_unread(&thread, mentioned);
        }

        state.history_mut(&thread).insert(message);
    }

    /// An edit or a delete changes what the sidebar should say, but only if it
    /// hit the newest message in the thread.
    fn refresh_preview(&mut self, thread: &Thread) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if let Some(last) = state.history(thread).and_then(History::last).cloned() {
            state.record(thread, &last);
        }
    }

}
