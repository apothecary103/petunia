use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Context, EventEmitter};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, warn};

use petunia_config::Config;
use petunia_data::message::Range;
use petunia_data as data;
use petunia_data::{Fragment, History, MessageId, State, Thread};
use petunia_signal::command::Quoted;
use petunia_signal::{Command, Event};
use crate::ui::composer::Intent;

/// Everything the views read, and the one way they talk back to the Signal
/// worker. Views observe this entity rather than owning any of it.
pub struct Store {
    pub config: Arc<Config>,
    /// `None` until the worker reports which account it linked.
    state: Option<State>,
    commands: Option<UnboundedSender<Command>>,
    /// The window is up and clickable well before the worker reports in, so
    /// anything asked for in that gap waits here rather than being lost.
    queued: Vec<Command>,
    /// The linking QR, while there is one to show.
    pub link_url: Option<String>,
    pub link_failure: Option<String>,
    /// The conversation on screen. A message arriving here is read, not unread.
    active: Option<Thread>,
    /// What the details panel is looking at. `None` means the conversation
    /// itself, which is what opening the panel with nothing picked shows.
    focus: Option<Focus>,
}

/// Something worth inspecting, named by what it is rather than by which panel
/// happens to show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Person(uuid::Uuid),
}

/// What views react to. A repaint alone is `cx.notify()`; these are the moments
/// that need more than a repaint.
#[derive(Debug, Clone)]
pub enum StoreEvent {
    Linked,
    /// A page of history arrived for a thread, and the list has to keep its
    /// scroll position if it was a page of *older* messages.
    History { older: bool },
    /// Something was clicked that wants the details panel open.
    Inspecting,
    /// A right-click asked for a menu, at this point on screen.
    Menu {
        thread: Thread,
        at: gpui::Point<gpui::Pixels>,
    },
    /// What a search turned up, carrying the query it answers so a late result
    /// cannot replace a newer one.
    Found {
        query: String,
        hits: Vec<petunia_signal::db::search::Hit>,
    },
    Failed(String),
}

impl EventEmitter<StoreEvent> for Store {}

impl Store {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            state: None,
            commands: None,
            queued: Vec::new(),
            focus: None,
            link_url: None,
            link_failure: None,
            active: None,
        }
    }

    pub fn state(&self) -> Option<&State> {
        self.state.as_ref()
    }

    pub fn state_mut(&mut self) -> Option<&mut State> {
        self.state.as_mut()
    }

    pub fn active(&self) -> Option<&Thread> {
        self.active.as_ref()
    }

    /// The conversation before or after this one in the sidebar's order.
    pub fn adjacent(&self, forward: bool) -> Option<Thread> {
        self.state
            .as_ref()?
            .index
            .cycle(self.active.as_ref(), forward)
            .cloned()
    }

    /// The next conversation owed attention, wrapping past the end.
    pub fn next_unread(&self) -> Option<Thread> {
        self.state
            .as_ref()?
            .index
            .next_unread(self.active.as_ref())
            .cloned()
    }

    pub fn focus(&self) -> Option<&Focus> {
        self.focus.as_ref()
    }

    /// Emitted rather than applied here, because whether the panel opens is the
    /// workspace's business, not the model's.
    pub fn inspect(&mut self, focus: Option<Focus>, cx: &mut Context<Self>) {
        self.focus = focus;
        cx.emit(StoreEvent::Inspecting);
        cx.notify();
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
        // Looking at a conversation is what reading it means, so the receipts it
        // owes go out now rather than waiting to be asked for.
        self.mark_read(thread.clone());

        self.active = Some(thread);
        // Opening a conversation is not a claim about whose profile you wanted.
        self.focus = None;
        cx.notify();
    }

    /// Raised rather than handled here: where a menu goes is the workspace's
    /// business, not the model's.
    pub fn open_menu(
        &mut self,
        thread: Thread,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        cx.emit(StoreEvent::Menu { thread, at });
    }

    /// Records what a menu chose about a conversation, on screen at once and on
    /// disk behind it.
    pub fn set_flags(
        &mut self,
        thread: Thread,
        flags: petunia_data::index::Flags,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.state.as_mut() {
            state.index.set_flags(&thread, flags.clone());
        }
        self.send(Command::SetFlags { thread, flags });
        cx.notify();
    }

    pub fn flags(&self, thread: &Thread) -> petunia_data::index::Flags {
        self.state
            .as_ref()
            .map(|state| state.index.flags(thread))
            .unwrap_or_default()
    }

    /// Forgets a conversation: off the screen now, off the disk behind it.
    ///
    /// Local to this device. Signal's own "delete for me" travels through the
    /// Storage Service, which presage does not expose, so this cannot reach the
    /// phone -- and the conversation comes back the moment anyone says anything
    /// in it, because a new message is a new thread as far as the store is
    /// concerned.
    pub fn delete_thread(&mut self, thread: Thread, cx: &mut Context<Self>) {
        if let Some(state) = self.state.as_mut() {
            state.index.forget(&thread);
            // The loaded messages go too. Without this, reopening the contact
            // from the switcher would show the conversation still sitting in
            // memory, which reads as the delete having failed.
            state.histories.remove(&thread);
        }
        if self.active.as_ref() == Some(&thread) {
            self.active = None;
        }
        self.send(Command::DeleteThread { thread });
        cx.notify();
    }

    /// Sends what the composer built, and puts it on screen before the network
    /// has heard of it. The echo carries `Sending`, which the worker replaces
    /// with what actually happened.
    pub fn compose(
        &mut self,
        thread: Thread,
        body: String,
        ranges: Vec<Range>,
        attachments: Vec<PathBuf>,
        intent: Option<Intent>,
        cx: &mut Context<Self>,
    ) {
        let Some(aci) = self.state.as_ref().map(|state| state.aci) else {
            return;
        };
        let timestamp = now();

        // An edit replaces a bubble that already exists rather than adding one,
        // so it takes neither a new timestamp nor an echo of its own.
        if let Some(Intent::Edit { target }) = &intent {
            let target = *target;
            self.send(Command::EditMessage {
                thread: thread.clone(),
                target: target.timestamp,
                body: body.clone(),
                ranges: ranges.clone(),
                timestamp,
            });
            if let Some(state) = self.state.as_mut() {
                let edit = data::Message::written(
                    MessageId {
                        timestamp,
                        sender: aci,
                    },
                    body,
                    ranges,
                );
                state.history_mut(&thread).apply_edit(&target, edit, timestamp);
            }
            cx.notify();
            return;
        }

        let target = match &intent {
            Some(Intent::Reply { target, .. }) => Some(*target),
            _ => None,
        };
        let quote = target.and_then(|target| self.quoted(target));
        // The wire carries no thumbnail with a quote of ours, but the original is
        // right here -- so the echo shows the picture it is answering rather than
        // the word "Photo", which is what an incoming quote of media looks like.
        let still = target
            .and_then(|target| self.original(&target))
            .and_then(|message| message.attachments.first())
            .filter(|attached| matches!(attached.kind, data::attachment::Kind::Image { .. }))
            .cloned();

        let mut echo = data::Message::written(
            MessageId {
                timestamp,
                sender: aci,
            },
            body.clone(),
            ranges.clone(),
        );
        echo.status = Some(data::Status::Sending);
        echo.quote = quote.as_ref().map(|quoted| {
            Box::new(data::message::Quote {
                id: quoted.id,
                body: quoted.body.clone(),
                ranges: quoted.ranges.clone(),
                // `Quoted::body` is already `Message::summary`, so a reply to a
                // picture carries "Photo" as its text and needs nothing further
                // to say.
                media: None,
                thumbnail: still,
            })
        });
        for path in &attachments {
            let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            echo.attachments
                .push(petunia_data::attachment::from_path(path.clone(), size));
        }

        self.send(if attachments.is_empty() {
            Command::SendText {
                thread: thread.clone(),
                body,
                ranges,
                quote,
                timestamp,
            }
        } else {
            Command::SendAttachments {
                thread: thread.clone(),
                body,
                ranges,
                paths: attachments,
                quote,
                timestamp,
            }
        });

        if let Some(state) = self.state.as_mut() {
            state.record(&thread, &echo);
            state.history_mut(&thread).insert(echo);
        }
        cx.notify();
    }

    /// Sends a sticker. It goes on its own rather than alongside whatever is
    /// typed, because Signal has no way to carry both.
    pub fn send_sticker(
        &mut self,
        thread: Thread,
        chosen: crate::ui::composer::stickers::Chosen,
        cx: &mut Context<Self>,
    ) {
        let Some(aci) = self.state.as_ref().map(|state| state.aci) else {
            return;
        };
        let timestamp = now();

        let mut echo = data::Message::plain(
            MessageId {
                timestamp,
                sender: aci,
            },
            String::new(),
        );
        echo.status = Some(data::Status::Sending);
        echo.content = data::message::Content::Sticker(Box::new(data::message::Sticker {
            pack_id: chosen.pack_id.clone(),
            pack_key: Some(chosen.key.clone()),
            sticker_id: chosen.sticker_id,
            emoji: chosen.emoji.clone(),
            image: Some(petunia_data::attachment::from_path(chosen.path.clone(), 0)),
        }));

        self.send(Command::SendSticker {
            thread: thread.clone(),
            pack_id: chosen.pack_id,
            key: chosen.key,
            sticker_id: chosen.sticker_id,
            emoji: chosen.emoji,
            path: chosen.path,
            timestamp,
        });

        if let Some(state) = self.state.as_mut() {
            state.record(&thread, &echo);
            state.history_mut(&thread).insert(echo);
        }
        cx.notify();
    }

    /// The message an id names. Every thread, because a reply is composed against
    /// whatever is on screen and the id says nothing about which thread that is.
    fn original(&self, target: &MessageId) -> Option<&data::Message> {
        self.state
            .as_ref()?
            .histories
            .values()
            .find_map(|history| history.find(target))
    }

    /// A reply carries a snapshot of what it answers, because the recipient may
    /// not have the original.
    fn quoted(&self, target: MessageId) -> Option<Quoted> {
        let message = self.original(&target)?;

        Some(Quoted {
            id: target,
            body: message.summary(),
            ranges: message.ranges().to_vec(),
        })
    }

    /// Reacts to a message, or takes the reaction back when it is already ours.
    pub fn react(&mut self, thread: Thread, target: MessageId, emoji: String, cx: &mut Context<Self>) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let aci = state.aci;
        let mine = state
            .history(&thread)
            .and_then(|history| history.find(&target))
            .is_some_and(|message| {
                message
                    .reactions
                    .iter()
                    .any(|reaction| reaction.author == aci && reaction.emoji == emoji)
            });

        let timestamp = now();
        state.history_mut(&thread).apply_reaction(
            &target,
            data::Reaction {
                author: aci,
                emoji: emoji.clone(),
                timestamp,
            },
            mine,
        );
        self.send(Command::React {
            thread,
            target,
            emoji,
            remove: mine,
            timestamp,
        });
        cx.notify();
    }

    pub fn delete(&mut self, thread: Thread, target: MessageId, cx: &mut Context<Self>) {
        if let Some(state) = self.state.as_mut() {
            state.history_mut(&thread).apply_delete(&target);
        }
        self.send(Command::DeleteMessage {
            thread,
            target: target.timestamp,
            timestamp: now(),
        });
        cx.notify();
    }

    /// Owns up to everything unread in a thread: a receipt to each sender and a
    /// sync to our own other devices.
    pub fn mark_read(&mut self, thread: Thread) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let messages = state.unread_receipts(&thread);
        if messages.is_empty() {
            return;
        }
        self.send(Command::MarkRead { thread, messages });
    }

    pub fn send(&mut self, command: Command) {
        match &self.commands {
            Some(sender) => {
                let _ = sender.send(command);
            }
            None => self.queued.push(command),
        }
    }

    pub fn config_changed(&mut self, config: Arc<Config>, cx: &mut Context<Self>) {
        self.config = config;
        self.apply_config();
        cx.notify();
    }

    /// Preferences the model itself reads: the sidebar's order and what our own
    /// messages are called. A hot reload has to reach these rather than only the
    /// palette.
    fn apply_config(&mut self) {
        let sort = self.config.sidebar.sort;
        let show_own_name = self.config.messages.show_own_name;
        if let Some(state) = self.state.as_mut() {
            state.index.set_sort(sort);
            state.show_own_name = show_own_name;
        }
    }

    /// Routes one worker event into the model. The only place `State` is
    /// mutated from outside a view.
    pub fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Ready(sender) => {
                for command in self.queued.drain(..) {
                    let _ = sender.send(command);
                }
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
                self.apply_config();
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
            Event::StickerPacks(packs) => state.sticker_packs = packs,
            Event::Found { query, hits } => cx.emit(StoreEvent::Found { query, hits }),
            Event::Flags(flags) => {
                for (thread, flags) in flags {
                    state.index.set_flags(&thread, flags);
                }
            }
            Event::Poster { thread, id, path } => {
                state.history_mut(&thread).set_poster(&id, path);
            }
            // Reading elsewhere is still reading, so the badge here goes.
            Event::Read { thread, upto } => {
                state.index.clear_unread(&thread);
                state.history_mut(&thread).mark_unread_from(None);
                let _ = upto;
            }
            Event::Unread { thread, count } => state.index.set_unread(&thread, count),
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
            Event::Attachment {
                thread,
                id,
                blob,
                measured,
            } => {
                state.history_mut(&thread).set_blob(&id, blob, measured);
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
                // Opening a thread marks it read, but on the first open there is
                // nothing loaded yet to owe a receipt for, so the page arriving
                // is when the debt becomes knowable.
                if !older && self.active.as_ref() == Some(&thread) {
                    self.mark_read(thread);
                }
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

        let unread = message.sender() != state.aci && active.as_ref() != Some(&thread);
        if unread {
            let mentioned = message.mentions(state.aci);
            state.index.mark_unread(&thread, mentioned);
        }
        let sender = message.sender();
        let timestamp = message.timestamp();
        state.history_mut(&thread).insert(message);

        // Arriving in the conversation you are looking at is arriving read.
        if !unread && sender != state.aci {
            self.send(Command::MarkRead {
                thread,
                messages: vec![(sender, timestamp)],
            });
        }
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

/// Signal identifies a message by when it was sent, so this is an identity as
/// much as a clock reading.
fn now() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}
