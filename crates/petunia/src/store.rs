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
    /// Whether the window is the one in front. Reading a message is looking at
    /// it, and nobody is looking at a window that is behind another one -- so
    /// this decides both whether an arriving message counts as unread and
    /// whether it owes a read receipt. Only the window knows, so it says.
    frontmost: bool,
    /// What the details panel is looking at. `None` means the conversation
    /// itself, which is what opening the panel with nothing picked shows.
    focus: Option<Focus>,
    /// This account's username and the `signal.me` link it confirmed with,
    /// once the worker has reported one. `None` until asked for, or after it
    /// is deleted.
    pub username: Option<(String, String)>,
    /// This account's own number, in +E.164. `None` until linking finishes.
    pub phone_number: Option<String>,
    /// The stickers kept to hand, which are a reference into the installed
    /// packs rather than stickers of their own.
    pub favourites: crate::favourites::Favourites,
    /// Packs that have been asked for but not installed, by pack id, and what
    /// came back: what the sheet for a sticker from a pack this account does not
    /// have shows. Present and `None` means the fetch is still out, which is
    /// also what stops it being asked for again every frame.
    previews: std::collections::HashMap<Vec<u8>, Option<Result<data::stickers::Pack, String>>>,
    /// Installed packs there is nothing to draw of, because none of their
    /// stickers ever arrived. Kept so the picker can say which of the two empty
    /// pickers it is: an account with no packs, or an account whose packs did
    /// not download.
    unreadable_packs: usize,
    /// How much has been said, per conversation, once it has been counted.
    /// Empty until something asks: it is a full pass over the store, and it is
    /// behind a preference that is off by default.
    pub counts: std::collections::HashMap<Thread, petunia_signal::db::messages::Tally>,
    /// Whether the count is out for an answer. A panel rebuilt every frame asks
    /// once rather than once a frame.
    counting: bool,
}

/// Everything a message being sent carries. One value rather than five
/// positional arguments, for the reason the worker's own `Composed` is one: a
/// caller passing a body and a caption and two booleans by position is a caller
/// who can swap two of them and be told nothing.
pub struct Composing {
    pub body: String,
    /// Signal carries formatting as offsets over the body, not as markup.
    pub ranges: Vec<Range>,
    pub attachments: Vec<PathBuf>,
    /// Whether this is answering a message or replacing one.
    pub intent: Option<Intent>,
    /// Whether the attachments are a recording rather than files somebody
    /// picked. Only the composer knows, and only it can say.
    pub voice: bool,
}

/// What is known about a pack that has been asked about but not installed.
#[derive(Debug, Clone, Copy)]
pub enum Previewed<'a> {
    Reading,
    Read(&'a data::stickers::Pack),
    Failed(&'a str),
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
    /// This device is unlinked and the local account cleared; the window
    /// should show the linking screen again.
    LoggedOut,
    /// What a new-chat lookup resolved to, carrying the query it answers so a
    /// late answer cannot replace a newer one.
    LookedUp {
        query: String,
        found: Option<petunia_data::Contact>,
    },
    /// A conversation the model has decided should be on screen -- a group just
    /// created, which nothing clicked on. Which pane shows it is the workspace's
    /// business, so this is raised rather than applied.
    Opened(Thread),
    /// Something the desktop should be told about, unless `on_screen` -- the
    /// message arrived in the conversation being read, in the window in front,
    /// which is the one case a banner is certainly wrong.
    Notify {
        notice: crate::notify::Notice,
        on_screen: bool,
    },
    /// A short tone. Raised rather than played, because the audio device
    /// belongs to the window and the model has never heard of it.
    Sound(petunia_media::audio::Chime),
}

impl EventEmitter<StoreEvent> for Store {}

impl Store {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            state: None,
            commands: None,
            queued: Vec::new(),
            // The window takes the front at launch, and says so as soon as it
            // has one; assuming otherwise would leave the first messages of a
            // session unread in a conversation being read.
            frontmost: true,
            focus: None,
            link_url: None,
            link_failure: None,
            active: None,
            username: None,
            phone_number: None,
            favourites: crate::favourites::Favourites::load(),
            previews: std::collections::HashMap::new(),
            unreadable_packs: 0,
            counts: std::collections::HashMap::new(),
            counting: false,
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

        // Whether a *page* has been read, not whether a history exists. A message
        // arriving live builds a history out of nothing but itself, and treating
        // that as the thread having been loaded left everything already on disk
        // invisible -- which is what a conversation that had been talked in while
        // petunia was closed looked like: the handful of messages the receive
        // queue delivered, no way to scroll back, and the rest only after a
        // restart.
        let unseen = self.state.as_ref().is_none_or(|state| {
            state
                .history(&thread)
                .is_none_or(|history| !history.has_page())
        });
        if unseen {
            self.send(Command::load(thread.clone()));
        }
        if let Some(state) = self.state.as_mut() {
            state.index.clear_unread(&thread);
        }
        // Looking at a conversation is what reading it means, so the receipts it
        // owes go out now rather than waiting to be asked for.
        self.mark_read(thread.clone());
        // And a message that has been read is a message that has started
        // disappearing, which is Signal's own rule and the reason nothing
        // vanishes out of a conversation nobody has opened.
        self.start_expiry(&thread);

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

    /// Turns disappearing messages on or off for a conversation. Everybody in
    /// it is told, because the timer belongs to the conversation and not to
    /// this device — which is also why there is no local echo here: the update
    /// comes back as a line in the thread like any other message.
    pub fn set_expire_timer(&mut self, thread: Thread, seconds: u32, cx: &mut Context<Self>) {
        self.send(Command::SetExpireTimer {
            thread,
            seconds,
            timestamp: now(),
        });
        cx.notify();
    }

    /// Starts the clock on everything in this conversation that carries one and
    /// has not started already.
    ///
    /// Called when a conversation is read, which is Signal's own rule: a
    /// message begins disappearing when somebody has seen it, not when it
    /// lands. The worker ignores a message it has already started, so calling
    /// this every time the thread is opened does not keep granting it another
    /// hour.
    fn start_expiry(&mut self, thread: &Thread) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Some(history) = state.history(thread) else {
            return;
        };

        let messages: Vec<_> = history
            .messages()
            .iter()
            .filter_map(|message| {
                message
                    .expires_in
                    .filter(|seconds| *seconds > 0)
                    .map(|seconds| (message.id, seconds))
            })
            .collect();

        if !messages.is_empty() {
            self.send(Command::StartExpiry {
                thread: thread.clone(),
                messages,
            });
        }
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

    /// Unlinks this device and clears everything stored locally. There is no
    /// undo: the window falls back to the linking screen once the worker
    /// confirms it with `Event::LoggedOut`.
    pub fn log_out(&mut self) {
        self.send(Command::LogOut);
    }

    /// Reserves and confirms a username, replacing whatever this account had.
    pub fn set_username(&mut self, nickname: String) {
        self.send(Command::SetUsername { nickname });
    }

    pub fn delete_username(&mut self) {
        self.send(Command::DeleteUsername);
    }

    /// Resolves a username or a phone number to an account, so a conversation
    /// can be started with somebody who is not in the contact list.
    pub fn look_up(&mut self, query: String) {
        self.send(Command::LookUp { query });
    }

    /// Creates a group. The conversation opens when the worker answers with the
    /// master key that is the group's identity, not before: until the server has
    /// taken it there is no thread to open.
    pub fn create_group(&mut self, title: String, members: Vec<uuid::Uuid>) {
        self.send(Command::CreateGroup {
            title,
            members,
            timestamp: now(),
        });
    }

    /// Sets or clears the nickname and note shown for a contact. Applied here
    /// before the round trip, the way a flag or a delete already is, so the
    /// panel does not wait on Storage Service to show what was just typed.
    pub fn set_nickname(
        &mut self,
        contact: uuid::Uuid,
        name: Option<String>,
        note: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.state.as_mut() {
            state.set_nickname(contact, name.clone(), note.clone());
        }
        self.send(Command::SetNickname { contact, name, note });
        cx.notify();
    }

    /// Replaces the profile picture. Takes the bytes rather than a path: the
    /// picker already read them to show a preview, and reading the file twice
    /// would only risk it having changed between the two.
    pub fn set_avatar(&mut self, bytes: Vec<u8>) {
        self.send(Command::SetAvatar { bytes });
    }

    /// Sends what the composer built, and puts it on screen before the network
    /// has heard of it. The echo carries `Sending`, which the worker replaces
    /// with what actually happened.
    pub fn compose(&mut self, thread: Thread, composed: Composing, cx: &mut Context<Self>) {
        let Composing {
            body,
            ranges,
            attachments,
            intent,
            voice,
        } = composed;

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
                voice,
            }
        });

        if let Some(state) = self.state.as_mut() {
            state.record(&thread, &echo);
            state.history_mut(&thread).insert(echo);
        }
        // On the press rather than on the acknowledgement. What the sound
        // answers is "did that go", and an answer that waits for the server is
        // an answer that arrives after the next thing has been typed.
        if self.config.notifications.sounds {
            cx.emit(StoreEvent::Sound(petunia_media::audio::Chime::Sent));
        }
        cx.notify();
    }

    /// Sends a message on to another conversation.
    ///
    /// A new message rather than anything marked as a forward: Signal's own
    /// forward carries a flag that presage does not expose, so this is the honest
    /// version of it -- the words and the files, sent again. Attachments have to
    /// be on disk, because sending one means reading it.
    pub fn forward(&mut self, target: MessageId, thread: Thread, cx: &mut Context<Self>) {
        use petunia_data::attachment::Blob;

        let Some(message) = self.original(&target) else {
            return;
        };
        let body = message.text().unwrap_or_default().to_owned();
        let ranges = message.ranges().to_vec();
        let attachments: Vec<PathBuf> = message
            .attachments
            .iter()
            .filter_map(|attached| match &attached.blob {
                Blob::Cached(path) => Some(path.clone()),
                _ => None,
            })
            .collect();

        // Said rather than silently dropped: a message carrying only a picture
        // that was never downloaded has nothing to send on, and a forward that
        // does nothing looks like a broken menu item.
        if body.trim().is_empty() && attachments.is_empty() {
            cx.emit(StoreEvent::Failed(
                "nothing to forward: download the attachment first".into(),
            ));
            return;
        }

        self.compose(
            thread,
            Composing {
                body,
                ranges,
                attachments,
                intent: None,
                voice: false,
            },
            cx,
        );
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

    /// Keeps a sticker to hand, or lets it go again.
    pub fn keep_sticker(&mut self, pack_id: &[u8], sticker_id: u32, cx: &mut Context<Self>) {
        self.favourites.toggle(pack_id, sticker_id);
        cx.notify();
    }

    /// Every installed pack, which is what the picker draws.
    pub fn sticker_packs(&self) -> &[data::stickers::Pack] {
        self.state
            .as_ref()
            .map(|state| state.sticker_packs.as_slice())
            .unwrap_or_default()
    }

    /// Installed packs with nothing in them to draw. What the picker says when
    /// it has nothing to show depends on this: with none of these, an empty
    /// picker is an account that has never added a pack.
    pub fn unreadable_packs(&self) -> usize {
        self.unreadable_packs
    }

    /// The installed pack a given id names, if this account has it.
    pub fn installed(&self, pack_id: &[u8]) -> Option<&data::stickers::Pack> {
        self.sticker_packs().iter().find(|pack| pack.id == pack_id)
    }

    /// What is known about a pack this account does not have. `None` for one
    /// nothing has been asked about.
    pub fn preview(&self, pack_id: &[u8]) -> Option<Previewed<'_>> {
        self.previews.get(pack_id).map(|answer| match answer {
            None => Previewed::Reading,
            Some(Ok(pack)) => Previewed::Read(pack),
            Some(Err(why)) => Previewed::Failed(why),
        })
    }

    /// Asks for a pack this account does not have. Asked for once: the answer,
    /// and the wait for it, are both remembered, so a view that calls this every
    /// frame costs one fetch.
    pub fn preview_pack(&mut self, pack_id: Vec<u8>, key: Vec<u8>) {
        if self.previews.contains_key(&pack_id) {
            return;
        }
        self.previews.insert(pack_id.clone(), None);
        self.send(Command::PreviewStickerPack { pack_id, key });
    }

    /// The message an id names, for the views that are shown one rather than
    /// drawing the thread it is in.
    pub fn find(&self, target: &MessageId) -> Option<&data::Message> {
        self.original(target)
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

    /// Withdraws a message from everybody who got it. The tombstone goes up here
    /// straight away, because that is what every recipient is about to see.
    pub fn delete_for_everyone(
        &mut self,
        thread: Thread,
        target: MessageId,
        cx: &mut Context<Self>,
    ) {
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

    /// Drops a message from this account. Taken out of the history rather than
    /// replaced by a tombstone: nobody else was told, so there is nothing for a
    /// line of text to report.
    pub fn delete_for_me(&mut self, thread: Thread, target: MessageId, cx: &mut Context<Self>) {
        if let Some(state) = self.state.as_mut() {
            state.history_mut(&thread).remove(&target);
        }
        self.send(Command::DeleteForMe { thread, target });
        cx.notify();
    }

    /// Blocks or unblocks somebody. Applied here at once and sent on: the whole
    /// of blocking is a flag in Storage Service, and the round trip that writes
    /// it is not something a menu should appear to hang on.
    pub fn set_blocked(&mut self, contact: uuid::Uuid, blocked: bool, cx: &mut Context<Self>) {
        if let Some(state) = self.state.as_mut() {
            state.set_blocked(contact, blocked);
        }
        self.send(Command::SetBlocked { contact, blocked });
        cx.notify();
    }

    /// Creates a poll, and puts it on screen before the network has heard of
    /// it -- the same optimistic echo `compose` gives a text message.
    pub fn send_poll(
        &mut self,
        thread: Thread,
        question: String,
        options: Vec<String>,
        allow_multiple: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(aci) = self.state.as_ref().map(|state| state.aci) else {
            return;
        };
        let timestamp = now();

        let mut echo = data::Message::new(
            MessageId { timestamp, sender: aci },
            data::message::Content::Poll(Box::new(data::message::Poll {
                question: question.clone(),
                options: options.clone(),
                allow_multiple,
                ballots: Vec::new(),
                terminated: false,
            })),
        );
        echo.status = Some(data::Status::Sending);

        self.send(Command::SendPoll {
            thread: thread.clone(),
            question,
            options,
            allow_multiple,
            timestamp,
        });

        if let Some(state) = self.state.as_mut() {
            state.record(&thread, &echo);
            state.history_mut(&thread).insert(echo);
        }
        cx.notify();
    }

    /// Casts (or changes) this reader's own ballot. `chosen` is the whole set
    /// of options that should end up checked, since Signal's vote replaces a
    /// ballot rather than toggling one option in it.
    pub fn vote_poll(
        &mut self,
        thread: Thread,
        target: MessageId,
        chosen: Vec<u32>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let aci = state.aci;
        let count = state
            .history(&thread)
            .and_then(|history| history.find(&target))
            .and_then(|message| match &message.content {
                data::message::Content::Poll(poll) => poll.ballot_for(aci),
                _ => None,
            })
            .map(|ballot| ballot.count)
            .unwrap_or(0)
            + 1;

        state.history_mut(&thread).apply_poll_vote(&target, data::message::Ballot {
            voter: aci,
            option_indexes: chosen.clone(),
            count,
        });
        self.send(Command::VotePoll {
            thread,
            target,
            option_indexes: chosen,
            count,
            timestamp: now(),
        });
        cx.notify();
    }

    pub fn terminate_poll(&mut self, thread: Thread, target: MessageId, cx: &mut Context<Self>) {
        if let Some(state) = self.state.as_mut() {
            state.history_mut(&thread).apply_poll_terminate(&target);
        }
        self.send(Command::TerminatePoll {
            thread,
            target: target.timestamp,
            timestamp: now(),
        });
        cx.notify();
    }

    /// Whether the window is the one in front. Coming back to it is coming back
    /// to whatever conversation was open, so the receipts that went unowed while
    /// petunia was behind something else go now.
    pub fn front(&mut self, frontmost: bool, cx: &mut Context<Self>) {
        if self.frontmost == frontmost {
            return;
        }
        self.frontmost = frontmost;
        if let Some(thread) = self.active.clone().filter(|_| frontmost) {
            if let Some(state) = self.state.as_mut() {
                state.index.clear_unread(&thread);
            }
            self.mark_read(thread);
            cx.notify();
        }
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
            Event::Linked { aci, phone_number } => {
                self.link_url = None;
                self.link_failure = None;
                self.state = Some(State::new(aci));
                self.phone_number = Some(phone_number);
                self.apply_config();
                cx.emit(StoreEvent::Linked);
            }
            Event::Error(message) if self.state.is_none() => {
                error!(%message, "signal error while linking");
                self.link_failure = Some(message);
            }
            Event::LoggedOut => {
                self.state = None;
                self.username = None;
                self.link_url = None;
                self.link_failure = None;
                self.active = None;
                cx.emit(StoreEvent::LoggedOut);
            }
            Event::Username(username) => {
                self.username = username;
            }
            // Not part of the account's state: a pack that was read and not
            // added is a sheet's business and nothing else's.
            Event::StickerPackPreview { pack_id, pack } => {
                self.previews.insert(pack_id, Some(pack));
            }
            Event::Counts(counts) => {
                self.counts = counts;
                self.counting = false;
            }
            event => self.apply_to_state(event, cx),
        }
        cx.notify();
    }

    /// Asks for the tally. One request at a time: a panel is rebuilt every
    /// frame, and a full pass over the store per frame is a full pass over the
    /// store per frame.
    pub fn count_messages(&mut self) {
        if self.counting {
            return;
        }
        self.counting = true;
        self.send(Command::CountMessages);
    }

    /// Everything said on this account, whichever conversation it was in.
    pub fn total_count(&self) -> petunia_signal::db::messages::Tally {
        self.counts
            .values()
            .fold(Default::default(), |total, tally| {
                petunia_signal::db::messages::Tally {
                    sent: total.sent + tally.sent,
                    received: total.received + tally.received,
                }
            })
    }

    fn apply_to_state(&mut self, event: Event, cx: &mut Context<Self>) {
        let Some(state) = self.state.as_mut() else {
            warn!(?event, "event arrived before linking; dropped");
            return;
        };

        match event {
            Event::Contacts { contacts, groups } => state.contacts_updated(contacts, groups),
            Event::StickerPacks { packs, unreadable } => {
                state.sticker_packs = packs;
                self.unreadable_packs = unreadable;
            }
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
            Event::Nickname { uuid, name, note } => state.set_nickname(uuid, name, note),
            Event::Blocked { uuid, blocked } => state.set_blocked(uuid, blocked),
            // Either this device asked for it, or another one did and the sync
            // told us. Both mean the same thing here.
            Event::Forgotten { thread, target } => {
                state.history_mut(&thread).remove(&target);
            }
            Event::ExpireTimers(timers) => state.set_expire_timers(timers),
            Event::Avatar { thread, path } => {
                state.avatars.insert(thread, path);
            }
            Event::AvatarUpdated(path) => {
                state.avatars.insert(Thread::Contact(petunia_data::ContactId::Aci(state.aci)), path);
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
            Event::Preview { thread, preview } => state.record_preview(&thread, preview),
            Event::Activity { thread, at } => state.record_activity(&thread, at),
            Event::History {
                thread,
                messages,
                more,
                covered,
                older,
            } => {
                if !older
                    && let Some(last) = messages.last().cloned()
                {
                    state.record(&thread, &last);
                }
                let history = state.history_mut(&thread);
                if older {
                    history.prepend(messages, more, covered);
                } else {
                    history.merge(messages, more, covered);
                }
                cx.emit(StoreEvent::History { older });
                // Opening a thread marks it read, but on the first open there is
                // nothing loaded yet to owe a receipt for, so the page arriving
                // is when the debt becomes knowable.
                if !older && self.frontmost && self.active.as_ref() == Some(&thread) {
                    self.mark_read(thread);
                }
            }
            Event::MessageStatus { timestamps, status } => {
                let aci = state.aci;
                for history in state.histories.values_mut() {
                    history.apply_status(&timestamps, aci, status);
                }
                // The sidebar shows the same ticks on the line it draws, so it
                // has to hear the same receipt: the list is not built from the
                // histories, and a thread that has never been opened has none.
                state.index.apply_status(&timestamps, status);
            }
            Event::Fragment {
                thread,
                fragment,
                order,
            } => self.fragment(thread, fragment, order, cx),
            Event::Error(message) => {
                error!(%message, "signal error");
                cx.emit(StoreEvent::Failed(message));
            }
            Event::LookedUp { query, found } => {
                if let Some(contact) = found.clone() {
                    // Recorded before the panel is told, so activating the thread
                    // it names finds a name for it rather than a bare uuid.
                    state.record_contact(contact);
                }
                cx.emit(StoreEvent::LookedUp { query, found });
            }
            // Opening it is the workspace's business, not the model's, and the
            // thread is already in the index by the time this arrives.
            Event::GroupCreated { thread } => cx.emit(StoreEvent::Opened(thread)),
            Event::Ready(_)
            | Event::LinkUrl(_)
            | Event::Linked { .. }
            | Event::LoggedOut
            | Event::StickerPackPreview { .. }
            | Event::Counts(_)
            | Event::Username(_) => {}
        }
    }

    fn fragment(
        &mut self,
        thread: Thread,
        fragment: Fragment,
        order: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        // Blocking somebody has to mean not seeing what they send, or it is a
        // flag in Storage Service and nothing else. Dropped here rather than in
        // the worker because this is the one funnel every arriving row goes
        // through -- and dropped rather than stored and hidden, since a filter
        // over the history would be a filter every view had to remember to apply.
        if let Some(sender) = fragment.sender()
            && state.is_blocked(sender)
        {
            return;
        }

        match fragment {
            Fragment::Message(message) => self.message(thread, message, cx),
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
            Fragment::PollVote { target, ballot } => {
                state.history_mut(&thread).apply_poll_vote(&target, ballot);
            }
            Fragment::PollTerminate { target } => {
                state.history_mut(&thread).apply_poll_terminate(&target);
            }
            Fragment::Ignored => {}
        }
    }

    fn message(&mut self, thread: Thread, message: data::Message, cx: &mut Context<Self>) {
        let active = self.active.clone();
        let settings = self.config.notifications.clone();
        let Some(state) = self.state.as_mut() else {
            return;
        };

        state.record(&thread, &message);

        // Whether anybody is actually looking at this message, which is one
        // question rather than two: the conversation has to be the open one and
        // the window has to be in front. Being the open one alone is what it
        // asked before -- so a message arriving in the conversation last read
        // went unread nowhere and sent a read receipt back while petunia sat
        // behind somebody's browser.
        let on_screen = active.as_ref() == Some(&thread) && self.frontmost;
        let notice = crate::notify::wanted(
            &settings,
            state,
            &thread,
            &message,
            false,
            now(),
        );
        // A sound whether or not the conversation is on screen: it is the
        // answer to "did something come in", which is a question you have with
        // the thread in front of you as much as without it.
        let audible = crate::notify::audible(&settings, state, &thread, &message, now());

        let unread = message.sender() != state.aci && !on_screen;
        if unread {
            let mentioned = message.mentions(state.aci);
            state.index.mark_unread(&thread, mentioned);
        }
        let sender = message.sender();
        let timestamp = message.timestamp();
        // Arriving in the conversation being read is arriving read, which for a
        // message that carries a timer is when its clock starts. Without this
        // one landing in the open thread would sit there until the reader left
        // and came back, which is the one moment `activate` runs again.
        let started = (!unread && sender != state.aci)
            .then(|| message.expires_in.filter(|seconds| *seconds > 0))
            .flatten()
            .map(|seconds| (message.id, seconds));
        let ours = sender == state.aci;
        state.history_mut(&thread).insert(message);

        if let Some(started) = started {
            self.send(Command::StartExpiry {
                thread: thread.clone(),
                messages: vec![started],
            });
        }

        if let Some(notice) = notice {
            cx.emit(StoreEvent::Notify { notice, on_screen });
        }
        if audible {
            cx.emit(StoreEvent::Sound(petunia_media::audio::Chime::Received));
        }

        // Arriving in the conversation somebody is looking at is arriving read.
        if !unread && !ours {
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
pub fn now() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}
