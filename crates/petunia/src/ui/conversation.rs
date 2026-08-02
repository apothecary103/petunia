use chrono::{Local, NaiveDate};
use gpui::prelude::*;
use gpui::{
    Context, Entity, ListAlignment, ListState, SharedString, Window, div, list, px,
};

use super::composer::Composer;
use super::kit;
use super::message::act::{Act, Dispatch};
use super::message::group;
use super::message::run::Run;
use petunia_media::audio::{Playback, Player};
use petunia_config::Theme;
use petunia_config::messages::{Layout, Reply, Spacing, Timestamps};
use petunia_data::{Message, MessageId, State, Thread};
use petunia_signal::Command;
use crate::store::{Focus, Store};
use crate::theme::ActivePalette;

/// A picture was asked for full size. The workspace owns the viewer, because it
/// covers more than the conversation column.
#[derive(Debug, Clone)]
pub struct Viewing(pub std::path::PathBuf);

impl gpui::EventEmitter<Viewing> for Conversation {}

/// A menu was asked for. The workspace owns it for the same reason it owns the
/// viewer: a menu opened near the bottom of the column has to be able to flip
/// above the pointer, which means knowing about more than the column.
///
/// The items are in a cell because an event is handed out by reference and
/// these are closures -- there is nothing to clone them from. Whoever handles it
/// takes them; a second handler would find an empty menu and draw nothing, which
/// is the right answer for a menu that has already been raised.
pub struct Raise {
    items: std::cell::RefCell<Vec<crate::ui::menu::Item>>,
    pub at: gpui::Point<gpui::Pixels>,
}

impl Raise {
    fn new(items: Vec<crate::ui::menu::Item>, at: gpui::Point<gpui::Pixels>) -> Self {
        Self {
            items: std::cell::RefCell::new(items),
            at,
        }
    }

    pub fn take(&self) -> Vec<crate::ui::menu::Item> {
        self.items.take()
    }
}

impl gpui::EventEmitter<Raise> for Conversation {}

/// A message is to be sent on to another conversation. The workspace owns the
/// picker, because choosing where means looking at the whole list.
#[derive(Debug, Clone)]
pub struct Forwarding(pub MessageId);

/// What the wire actually said, asked for. The workspace owns the sheet for the
/// same reason it owns every other one.
#[derive(Debug, Clone)]
pub struct Inspecting(pub MessageId);

/// A poll was asked for, in this conversation. The workspace owns the dialog
/// that builds one, same as every other one it hosts.
#[derive(Debug, Clone)]
pub struct Polling(pub Thread);

/// A sticker was clicked, and wants looking at.
#[derive(Debug, Clone)]
pub struct Stickering(pub Box<petunia_data::message::Sticker>);

/// A message is to be deleted, one way or the other. Which way is asked by a
/// dialog the workspace owns, like every other one.
#[derive(Debug, Clone)]
pub struct Deleting(pub MessageId);

/// Somebody is to be given a nickname.
#[derive(Debug, Clone)]
pub struct Naming(pub uuid::Uuid);

/// Somebody is to be blocked, once they have been asked about.
#[derive(Debug, Clone)]
pub struct Blocking(pub uuid::Uuid);

impl gpui::EventEmitter<Forwarding> for Conversation {}
impl gpui::EventEmitter<Inspecting> for Conversation {}
impl gpui::EventEmitter<Polling> for Conversation {}
impl gpui::EventEmitter<Stickering> for Conversation {}
impl gpui::EventEmitter<Deleting> for Conversation {}
impl gpui::EventEmitter<Naming> for Conversation {}
impl gpui::EventEmitter<Blocking> for Conversation {}

/// How far beyond the window the list measures. A run is tall and its height is
/// only known once it is built, so a scroll that outruns the measured region
/// stutters while it catches up; a window's worth of slack is enough that a
/// flick never does.
const OVERDRAW: f32 = 800.0;

/// How far above the plus its menu is put. Roughly the two rows it holds, so it
/// opens over the composer rather than off the bottom of the window -- `Menu`
/// itself flips a menu that would run off an edge, and the edge it would run off
/// here is the one it is anchored to.
const MORE_MENU: f32 = 76.0;

/// How long a message a search jumped to stays lit. Long enough to find it on
/// the page, short enough that it is not still lit when you come back.
const REVEAL_FOR: std::time::Duration = std::time::Duration::from_millis(2500);

/// How many pages back a search hit is worth looking for. One click must not
/// turn into an unbounded download of a conversation with years behind it.
const REVEAL_PAGES: u32 = 20;

/// How long a message stays lit after its text has been copied. Long enough to
/// be seen, short enough not to be mistaken for a state the message is in.
const COPIED_FOR: std::time::Duration = std::time::Duration::from_millis(1200);

/// A message a search asked to be taken to.
#[derive(Clone, Copy)]
struct Reveal {
    target: MessageId,
    /// Pages still worth fetching before giving up on finding it.
    budget: u32,
}

/// Hands a file to whatever the system opens it with.
fn open(path: &std::path::Path) {
    if let Err(error) = open::that_detached(path) {
        tracing::warn!(%error, path = %path.display(), "could not open the file");
    }
}

/// The focused conversation: the message list, the composer beneath it, and the
/// one place every control drawn on a message is answered.
pub struct Conversation {
    store: Entity<Store>,
    composer: Entity<Composer>,
    player: Player,
    /// Only the rows on screen are built, so the cost of a scroll frame is the
    /// height of the window rather than the length of the thread. A plain
    /// scrolling `div` rebuilt and re-shaped every message in the history on
    /// every frame of every scroll.
    list: ListState,
    /// The thread the scroll position belongs to, so switching conversations
    /// starts at the newest message rather than wherever the last one was.
    anchored: Option<Thread>,
    /// The rows the list was last told about, and how many messages they were
    /// built from. Kept in full rather than counted, because the list addresses
    /// its scroll position *and every measured row height* by index, and a count
    /// cannot say which rows a repaint rewrote -- see `group::changed`.
    shown: Vec<group::Row>,
    messages: usize,
    /// Whether the next change arrived above what is on screen, which decides
    /// whether the rows already there have been renumbered.
    prepending: bool,
    /// Runs only while something is playing, because a repaint every tenth of a
    /// second for an idle window is not free.
    ticking: Option<gpui::Task<()>>,
    /// Ages out typing indicators. `State::typing` already filters by elapsed
    /// time, but nothing would ask it again, so an indicator whose "stopped" was
    /// lost would sit there until the next unrelated repaint.
    aging: Option<gpui::Task<()>>,
    /// What a search asked to be shown, while it is still being looked for or
    /// still lit.
    revealed: Option<Reveal>,
    /// The message whose text was just copied, and the task that stops saying so.
    copied: Option<u64>,
    confirming: Option<gpui::Task<()>>,
    /// Where to put the list once it has been told how many rows there are.
    /// Applied after `reconcile` rather than when the row is worked out, because
    /// scrolling to a row the list has not heard of yet clamps to the end.
    pending_scroll: Option<usize>,
    fading: Option<gpui::Task<()>>,
}

impl Conversation {
    pub fn new(
        store: Entity<Store>,
        player: Player,
        drafts: Vec<(Thread, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |this: &mut Self, store, cx| {
            if store.read(cx).state().is_some_and(State::anyone_typing) {
                this.age_typing(cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe_in(
            &store,
            window,
            |this: &mut Self, _, event: &crate::store::StoreEvent, _, cx| {
                // Which end the next rows arrive at. The list keeps the reader
                // where they are either way, but only if it is told where they
                // went in.
                if let crate::store::StoreEvent::History { older } = event {
                    this.prepending |= *older;
                    // A page arriving is when a search hit further back than the
                    // page it was looking in becomes findable.
                    if this.revealed.is_some() {
                        this.seek(cx);
                    }
                    cx.notify();
                }
            },
        )
        .detach();
        let composer = cx.new(|cx| Composer::new(store.clone(), drafts, window, cx));
        cx.subscribe_in(
            &composer,
            window,
            |this: &mut Self, _, _: &super::composer::RequestPoll, _, cx| {
                if let Some(thread) = this.store.read(cx).active().cloned() {
                    cx.emit(Polling(thread));
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &composer,
            window,
            |_: &mut Self, composer, more: &super::composer::RequestMore, _, cx| {
                let composer = composer.clone();
                let items = vec![
                    crate::ui::menu::Item::new("Photo or file…", {
                        let composer = composer.clone();
                        move |window: &mut Window, cx: &mut gpui::App| {
                            composer.update(cx, |composer, cx| composer.pick_files(window, cx));
                        }
                    })
                    .icon(gpui_component::IconName::Folder),
                    crate::ui::menu::Item::new("Poll…", {
                        let this = cx.entity();
                        move |_, cx: &mut gpui::App| {
                            this.update(cx, |this, cx| {
                                if let Some(thread) = this.store.read(cx).active().cloned() {
                                    cx.emit(Polling(thread));
                                }
                            });
                        }
                    })
                    .icon(gpui_component::IconName::ChartPie),
                ];
                // Above the click, because the composer sits at the bottom of the
                // window and a menu dropped from there has nowhere to go. `Menu`
                // flips it back if that puts it off the top.
                let at = gpui::point(more.0.x, more.0.y - px(MORE_MENU));
                cx.emit(Raise::new(items, at));
            },
        )
        .detach();
        cx.subscribe_in(
            &composer,
            window,
            |this: &mut Self, _, raised: &super::composer::RequestStickerMenu, _, cx| {
                let items = this.sticker_menu(&raised.chosen, cx);
                cx.emit(Raise::new(items, raised.at));
            },
        )
        .detach();
        Self {
            store,
            composer,
            player,
            // Bottom-anchored, like every chat log: with nothing scrolled it sits
            // at the newest message and stays there as messages arrive.
            list: ListState::new(0, ListAlignment::Bottom, px(OVERDRAW)),
            anchored: None,
            shown: Vec::new(),
            messages: 0,
            prepending: false,
            ticking: None,
            aging: None,
            revealed: None,
            copied: None,
            confirming: None,
            pending_scroll: None,
            fading: None,
        }
    }

    /// Takes the reader to a message, loading older pages until it turns up.
    /// Called when a search result is chosen, which is the only way to arrive
    /// somewhere that is not the bottom of the thread.
    pub fn reveal(&mut self, target: MessageId, cx: &mut Context<Self>) {
        self.revealed = Some(Reveal {
            target,
            budget: REVEAL_PAGES,
        });
        self.seek(cx);
    }

    /// Scrolls to whatever `revealed` names, or asks for the page behind the one
    /// loaded and waits to be called again.
    fn seek(&mut self, cx: &mut Context<Self>) {
        let Some(&Reveal { target, budget }) = self.revealed.as_ref() else {
            return;
        };
        let store = self.store.read(cx);
        let Some(thread) = store.active().cloned() else {
            return;
        };
        let group_within_ms = store.config.messages.group_within * 1000;
        // No page yet: the thread was only just opened, and the load that is
        // already on its way will call this again.
        let Some(history) = store.state().and_then(|state| state.history(&thread)) else {
            return;
        };

        let found = history
            .messages()
            .iter()
            .position(|message| message.id == target);

        match found {
            Some(at) => {
                let rows = group::rows(
                    history.messages(),
                    history.first_unread(),
                    group_within_ms,
                );
                // One past the row it is in: row zero is the load-older button.
                self.pending_scroll = rows
                    .iter()
                    .position(|row| row.covers(at))
                    .map(|row| row + 1);
                self.fade(cx);
            }
            None if budget > 0 && history.has_more() => {
                if let Some(reveal) = self.revealed.as_mut() {
                    reveal.budget = budget - 1;
                }
                self.load_older(thread, cx);
            }
            // Nowhere left to look, so nothing is lit rather than something
            // being lit forever.
            None => self.revealed = None,
        }
        cx.notify();
    }

    /// Puts the highlight out again. The scroll stays where it was put.
    fn fade(&mut self, cx: &mut Context<Self>) {
        self.fading = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVEAL_FOR).await;
            this.update(cx, |this, cx| {
                this.revealed = None;
                this.fading = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// One tick a second while anyone is typing, and none otherwise.
    fn age_typing(&mut self, cx: &mut Context<Self>) {
        if self.aging.is_some() {
            return;
        }
        self.aging = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let carry_on = this.update(cx, |this, cx| {
                    this.store.update(cx, |store, cx| {
                        if let Some(state) = store.state_mut() {
                            state.expire_typing();
                        }
                        cx.notify();
                        store.state().is_some_and(petunia_data::State::anyone_typing)
                    })
                });
                match carry_on {
                    Ok(true) => {}
                    _ => break,
                }
            }
            this.update(cx, |this, _| this.aging = None).ok();
        }));
    }

    /// Everything a control on a message can ask for, answered in one place.
    fn dispatch(&self, cx: &mut Context<Self>) -> Dispatch {
        let this = cx.entity();
        std::rc::Rc::new(move |act, window, cx| {
            this.update(cx, |this, cx| this.perform(act, window, cx));
        })
    }

    fn perform(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread) = self.store.read(cx).active().cloned() else {
            return;
        };

        match act {
            Act::Download { timestamp, id } => self.store.update(cx, |store, cx| {
                store.send(Command::DownloadAttachment {
                    thread,
                    timestamp,
                    id,
                });
                cx.notify();
            }),
            Act::Reply(target) => self.reply_to(target, window, cx),
            Act::Edit(target) => self.edit(target, window, cx),
            Act::React(target, emoji) => self
                .store
                .update(cx, |store, cx| store.react(thread, target, emoji, cx)),
            Act::Delete(target) => cx.emit(Deleting(target)),
            Act::VotePoll(target, chosen) => self
                .store
                .update(cx, |store, cx| store.vote_poll(thread, target, chosen, cx)),
            Act::TerminatePoll(target) => self
                .store
                .update(cx, |store, cx| store.terminate_poll(thread, target, cx)),
            Act::Copy(target) => self.copy(target, cx),
            Act::View(path) => cx.emit(Viewing(path)),
            Act::Save(path) => self.save(path, window, cx),
            Act::Open(path) => open(&path),
            Act::OpenLink(url) => {
                if let Err(error) = open::that_detached(&url) {
                    tracing::warn!(%error, %url, "could not open the link");
                }
            }
            Act::Play(path) => {
                self.player.toggle(path);
                self.follow_playback(cx);
            }
            // Clicking into the waveform of something that is not playing means
            // "play this, from here" -- without the first half it would silently
            // scrub whatever else happened to be running.
            Act::Seek(path, fraction) => {
                if !self.player.playback().is(&path) {
                    self.player.toggle(path);
                    self.follow_playback(cx);
                }
                self.player.seek(fraction);
            }
            Act::ShowSticker(sticker) => cx.emit(Stickering(sticker)),
            Act::Inspect(who) => self.inspect(who, cx),
            Act::Forward(target) => cx.emit(Forwarding(target)),
            Act::Raw(target) => cx.emit(Inspecting(target)),
            Act::Nickname(who) => cx.emit(Naming(who)),
            Act::Block(who, blocked) => match blocked {
                true => cx.emit(Blocking(who)),
                // No dialog: unblocking is the undo, and asking before an undo
                // is asking twice about one decision.
                false => self
                    .store
                    .update(cx, |store, cx| store.set_blocked(who, false, cx)),
            },
            Act::Menu(target, at) => self.open_menu(target, at, cx),
            Act::MenuFor(who, at) => {
                let blocked = self
                    .store
                    .read(cx)
                    .state()
                    .is_some_and(|state| state.is_blocked(who));
                let items = crate::ui::menu::message::person(who, blocked, &self.dispatch(cx));
                cx.emit(Raise::new(items, at));
            }
        }
    }

    /// The menu for a sticker in the picker: keep it to hand, or go and look at
    /// the pack it came from.
    fn sticker_menu(
        &self,
        chosen: &super::composer::stickers::Chosen,
        cx: &mut Context<Self>,
    ) -> Vec<crate::ui::menu::Item> {
        let kept = self
            .store
            .read(cx)
            .favourites
            .holds(&chosen.pack_id, chosen.sticker_id);

        let keep = {
            let store = self.store.clone();
            let pack_id = chosen.pack_id.clone();
            let sticker_id = chosen.sticker_id;
            crate::ui::menu::Item::new(
                match kept {
                    true => "Remove from favourites",
                    false => "Add to favourites",
                },
                move |_, cx: &mut gpui::App| {
                    store.update(cx, |store, cx| store.keep_sticker(&pack_id, sticker_id, cx));
                },
            )
            .icon(gpui_component::IconName::Heart)
            .checked(kept)
        };

        let show = {
            let this = cx.entity();
            let sticker = petunia_data::message::Sticker {
                pack_id: chosen.pack_id.clone(),
                pack_key: Some(chosen.key.clone()),
                sticker_id: chosen.sticker_id,
                emoji: chosen.emoji.clone(),
                image: Some(petunia_data::attachment::from_path(chosen.path.clone(), 0)),
            };
            crate::ui::menu::Item::new("Show pack…", move |_, cx: &mut gpui::App| {
                let sticker = sticker.clone();
                this.update(cx, |_, cx| cx.emit(Stickering(Box::new(sticker))));
            })
            .icon(gpui_component::IconName::GalleryVerticalEnd)
        };

        vec![keep, crate::ui::menu::Item::Separator, show]
    }

    /// The menu for a message, built from the message itself.
    fn open_menu(
        &mut self,
        target: MessageId,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let act = self.dispatch(cx);
        let store = self.store.read(cx);
        let Some((message, own)) = store.state().and_then(|state| {
            let message = state
                .histories
                .values()
                .find_map(|history| history.find(&target))?;
            Some((message, message.sender() == state.aci))
        }) else {
            return;
        };

        let items = crate::ui::menu::message::items(message, own, &act);
        cx.emit(Raise::new(items, at));
    }

    fn copy(&mut self, target: MessageId, cx: &mut Context<Self>) {
        let Some(text) = self
            .store
            .read(cx)
            .state()
            .and_then(|state| {
                state
                    .histories
                    .values()
                    .find_map(|history| history.find(&target))
            })
            .and_then(|message| message.text())
            .map(str::to_owned)
        else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));

        // A clipboard write is invisible, so the message says it happened: it
        // lights for a moment and the copy button becomes a check.
        self.copied = Some(target.timestamp);
        self.confirming = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            this.update(cx, |this, cx| {
                this.copied = None;
                this.confirming = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Saving is a copy to somewhere the user picks; the cached file stays where
    /// it is, because the conversation still needs it.
    fn save(&self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".into());
        let directory = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
        let asked = cx.prompt_for_new_path(&directory, Some(&name));

        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(target))) = asked.await else {
                return;
            };
            let copied = cx
                .background_spawn(async move { std::fs::copy(&path, &target) })
                .await;
            if let Err(error) = copied {
                tracing::warn!(%error, "could not save the attachment");
            }
        })
        .detach();
    }

    /// Tells the list exactly which rows this repaint rewrote.
    ///
    /// Which is the whole point: rows spliced in at the front carry the reader
    /// down with them, so what they were reading stays under the pointer, while
    /// rows appended at the back leave them where they are -- and leave a reader
    /// who was at the bottom at the bottom, because that is what a bottom-anchored
    /// list does with no scroll position of its own. Both fall out of naming the
    /// range rather than the count, and so does the case neither a count nor an
    /// end could describe: a page of older messages rewriting the rows it landed
    /// beside.
    ///
    /// Row zero is the sentinel and is always row zero, so everything here is
    /// offset past it.
    fn reconcile(&mut self, thread: &Thread, rows: &[group::Row], messages: usize) {
        if self.anchored.as_ref() != Some(thread) {
            self.anchored = Some(thread.clone());
            self.shown = rows.to_vec();
            self.messages = messages;
            self.prepending = false;
            self.list.reset(rows.len() + 1);
            // A voice note belongs to the conversation it was sent in, so it
            // does not follow you into the next one.
            self.player.stop();
            return;
        }

        let prepending = std::mem::take(&mut self.prepending);
        let was = std::mem::replace(&mut self.messages, messages);
        // Only a page of older messages renumbers the rows already there; a
        // message arriving at the back leaves every index alone.
        let shift = match prepending {
            true => messages.saturating_sub(was),
            false => 0,
        };

        let shown = std::mem::replace(&mut self.shown, rows.to_vec());
        let (rewritten, count) = group::changed(&shown, rows, shift);
        if rewritten.is_empty() && count == 0 {
            return;
        }

        // Where the reader is, before the indices move under them.
        let at = self.list.logical_scroll_top();
        self.list
            .splice(rewritten.start + 1..rewritten.end + 1, count);

        // `splice` carries a scroll position that sits *after* the range it
        // rewrote, which is every reader who was below the change. One inside it
        // has nowhere to be carried to and is left where they were -- at the top,
        // with the page they just asked for below them and the message they were
        // reading shoved off the bottom of the window. So is one above it, which
        // for a prepend is a reader sitting on the sentinel. The first row the
        // rewrite did not touch is the one thing both lists have in common, and
        // that is where they go.
        if shift > 0 && at.item_ix <= rewritten.end {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: rewritten.start + count + 1,
                offset_in_item: px(0.0),
            });
        }
    }

    /// A playing track has to repaint to move; an idle one must not.
    fn follow_playback(&mut self, cx: &mut Context<Self>) {
        if self.ticking.is_some() {
            return;
        }
        self.ticking = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let carry_on = this.update(cx, |this, cx| {
                    cx.notify();
                    this.player.playback().playing
                });
                match carry_on {
                    Ok(true) => {}
                    _ => break,
                }
            }
            this.update(cx, |this, _| this.ticking = None).ok();
        }));
    }

    pub fn composer(&self) -> &Entity<Composer> {
        &self.composer
    }

    /// Replies to a message, snapshotting what it says for the banner.
    pub fn reply_to(&mut self, target: MessageId, window: &mut Window, cx: &mut Context<Self>) {
        let summary = self
            .store
            .read(cx)
            .state()
            .and_then(|state| {
                state
                    .histories
                    .values()
                    .find_map(|history| history.find(&target))
            })
            .map(Message::summary)
            .unwrap_or_default();

        self.composer
            .update(cx, |composer, cx| composer.reply_to(target, summary, window, cx));
    }

    pub fn edit(&mut self, target: MessageId, window: &mut Window, cx: &mut Context<Self>) {
        let body = self
            .store
            .read(cx)
            .state()
            .and_then(|state| {
                state
                    .histories
                    .values()
                    .find_map(|history| history.find(&target))
            })
            .and_then(|message| message.text())
            .unwrap_or_default()
            .to_string();

        self.composer
            .update(cx, |composer, cx| composer.edit(target, body, window, cx));
    }

    /// Up on an empty composer edits the last thing you said, which is what
    /// every chat client trains you to expect.
    pub fn edit_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.composer.read(cx).is_empty(cx) {
            return;
        }
        let Some(target) = self.last_own_message(cx) else {
            return;
        };
        self.edit(target, window, cx);
    }

    pub fn reply_to_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let Some(target) = store
            .active()
            .and_then(|thread| store.state()?.history(thread))
            .and_then(|history| {
                history
                    .messages()
                    .iter()
                    .rev()
                    .find(|message| message.is_addressable())
            })
            .map(|message| message.id)
        else {
            return;
        };
        self.reply_to(target, window, cx);
    }

    fn last_own_message(&self, cx: &Context<Self>) -> Option<MessageId> {
        let store = self.store.read(cx);
        let state = store.state()?;
        let history = state.history(store.active()?)?;

        history
            .messages()
            .iter()
            .rev()
            .find(|message| message.sender() == state.aci && message.is_addressable())
            .map(|message| message.id)
    }

    fn inspect(&mut self, sender: uuid::Uuid, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.inspect(Some(Focus::Person(sender)), cx));
    }

    fn load_older(&mut self, thread: Thread, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            let Some(before) = store
                .state()
                .and_then(|state| state.history(&thread))
                .and_then(|history| history.oldest())
            else {
                return;
            };
            if store
                .state()
                .and_then(|state| state.history(&thread))
                .is_some_and(petunia_data::History::is_loading)
            {
                return;
            }
            store.send(Command::LoadThread {
                thread: thread.clone(),
                before: Some(before),
            });
            if let Some(state) = store.state_mut() {
                state.history_mut(&thread).set_loading(true);
            }
            cx.notify();
        });
    }
}

impl Render for Conversation {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let act = self.dispatch(cx);
        let playback = self.player.playback();
        let store = self.store.read(cx);

        let Some(thread) = store.active().cloned() else {
            return empty(&palette, "Pick a conversation to start reading.").into_any_element();
        };
        let Some(state) = store.state() else {
            return empty(&palette, "Still connecting…").into_any_element();
        };

        let layout = store.config.messages.layout;
        let replies = store.config.messages.replies;
        let spacing = store.config.messages.spacing();
        let timestamps = store.config.messages.timestamps;
        // The config counts seconds; message timestamps are milliseconds.
        let group_within_ms = store.config.messages.group_within * 1000;
        let max_image = (
            store.config.media.image_max_width,
            store.config.media.image_max_height,
        );

        let Some(history) = state.history(&thread).filter(|history| !history.is_empty()) else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(palette.background)
                .child(empty(&palette, "No messages here yet.").flex_1())
                .child(self.composer.clone())
                .into_any_element();
        };

        let rows = group::rows(history.messages(), history.first_unread(), group_within_ms);
        // Row zero is the top of the thread: the button that fetches the previous
        // page, or nothing once there is nothing behind it. Always a row, even
        // when it draws nothing, because a row that came and went would shift
        // every index behind it and take the reader's place with it.
        let older = history.has_more();
        let loading = history.is_loading();

        self.reconcile(&thread, &rows, history.messages().len());
        // After reconcile, so the list has heard of the row being asked for.
        if let Some(row) = self.pending_scroll.take() {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: row,
                offset_in_item: px(0.0),
            });
        }

        let rows = std::rc::Rc::new(rows);
        let frame = std::rc::Rc::new(Frame {
            thread: thread.clone(),
            palette: palette.clone(),
            highlights: cx.highlights().clone(),
            layout,
            replies,
            spacing,
            timestamps,
            max_image,
            playback,
            revealed: self.revealed.as_ref().map(|reveal| reveal.target.timestamp),
            latest_own: history
                .messages()
                .iter()
                .rev()
                .find(|message| message.sender() == state.aci && message.status.is_some())
                .map(Message::timestamp),
            copied: self.copied,
            act,
        });
        let store = self.store.clone();
        let target = thread.clone();
        let this = cx.entity();

        let list = list(self.list.clone(), move |index, _window, cx| {
            let Some(row) = index.checked_sub(1) else {
                if !older {
                    return div().into_any_element();
                }
                // The top of the thread being built is the top of the thread
                // being approached: `list` only builds what is on screen and the
                // overdraw around it, so this row is the reader arriving. Asking
                // for the page here is what makes reading backwards continuous
                // rather than a button to find every screenful.
                //
                // Deferred, because this runs inside the layout that is asking
                // for the row, and the load mutates the very history it is
                // reading.
                if !loading {
                    let this = this.clone();
                    let target = target.clone();
                    cx.defer(move |cx| {
                        this.update(cx, |this, cx| this.load_older(target.clone(), cx));
                    });
                }
                return loading_older(&frame.palette, spacing);
            };

            // Read back out of the store rather than captured: a row addresses
            // the history by position, and the history is the store's.
            let Some(state) = store.read(cx).state() else {
                return div().into_any_element();
            };
            let Some(history) = state.history(&frame.thread) else {
                return div().into_any_element();
            };

            rows.get(row)
                .and_then(|row| frame.row(row, history.messages(), state))
                // Each row carries the space above it, since a list has no gap of
                // its own to set.
                .map(|row| div().pt(px(spacing.between_runs)).child(row).into_any_element())
                .unwrap_or_else(|| div().into_any_element())
        })
        .flex_1()
        .w_full()
        // Vertical only. gpui's `list` takes each row's origin from the left edge
        // of its own bounds and never reads `padding.left`, so side padding here
        // does nothing at all -- which is why it belongs to the wrapper below.
        // The top and bottom are honoured, and want to be inside the scroller.
        .py(px(spacing.padding_y));

        div()
            .id("conversation")
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.background)
            // Captured, so it runs before the words themselves hear the click:
            // pressing the mouse anywhere in the column puts out whatever was
            // lit, and a press that landed on some text then lights its own.
            // Ordering the two as bubble handlers would leave which of them won
            // to the order they happened to be painted in.
            .capture_any_mouse_down(|_, _, cx| super::selection::clear(cx))
            .on_drop(cx.listener(
                |this: &mut Self, paths: &gpui::ExternalPaths, _, cx| {
                    // One message carrying everything, rather than one message
                    // per file: the platform delivers them together and that is
                    // how they were meant.
                    let paths = paths.paths().to_vec();
                    this.composer
                        .clone()
                        .update(cx, |composer, cx| composer.attach(paths, cx));
                },
            ))
            .child(
                kit::column()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .px(px(spacing.padding_x))
                    .child(list),
            )
            .child(self.composer.clone())
            .into_any_element()
    }
}

fn empty(palette: &Theme, message: &'static str) -> gpui::Div {
    div()
        .size_full()
        .flex().flex_col()
        .items_center()
        .justify_center()
        .bg(palette.background)
        .text_color(palette.text_muted)
        .child(message)
}

/// Everything the rows need that is not the messages themselves. Owned rather
/// than borrowed: the list builds a row when it lays one out, which is after the
/// render that decided what the rows were has returned.
struct Frame {
    /// The thread it was said in, for what the group says about the sender.
    thread: Thread,
    palette: Theme,
    highlights: std::sync::Arc<gpui_component::highlighter::HighlightTheme>,
    layout: Layout,
    replies: Reply,
    spacing: Spacing,
    timestamps: Timestamps,
    max_image: (f32, f32),
    playback: Playback,
    /// The message a search jumped to, lit until the next one is asked for.
    revealed: Option<u64>,
    /// The newest message of this account's in the thread, which is the one the
    /// standard layout marks. Worked out once per repaint rather than per row: a
    /// scan of the history in every row of every frame is the cost the sidebar's
    /// previews were moved off the render path to avoid.
    latest_own: Option<u64>,
    /// The message whose text was just copied.
    copied: Option<u64>,
    act: Dispatch,
}

impl Frame {
    /// One row, or nothing when it addresses a message that is no longer there --
    /// the rows were worked out against the history as it was, and a delete can
    /// land between then and the layout that asks for this.
    fn row(
        &self,
        row: &group::Row,
        messages: &[Message],
        state: &State,
    ) -> Option<gpui::AnyElement> {
        Some(match row {
            group::Row::Day(date) => day_separator(*date, &self.palette, self.spacing),
            group::Row::UnreadMarker => unread_marker(&self.palette, self.spacing),
            group::Row::Update(index) => {
                update_line(messages.get(*index)?, &self.palette, self.spacing)
            }
            group::Row::Run { sender, messages: range } => {
                let run = messages.get(range.clone())?;
                if run.is_empty() {
                    return None;
                }
                Run {
                    sender: *sender,
                    messages: run,
                    state,
                    thread: &self.thread,
                    theme: &self.palette,
                    highlights: &self.highlights,
                    layout: self.layout,
                    replies: self.replies,
                    spacing: self.spacing,
                    timestamps: self.timestamps,
                    max_image: self.max_image,
                    playback: &self.playback,
                    revealed: self.revealed,
                    latest_own: self.latest_own,
                    copied: self.copied,
                    act: &self.act,
                }
                .render()
                .into_any_element()
            }
        })
    }
}

/// The top of the list while there is more behind it. It says what is happening
/// and is not a control: reaching it is what asks for the page, so a button here
/// would be a second way to ask for something already on its way.
fn loading_older(palette: &Theme, spacing: Spacing) -> gpui::AnyElement {
    div()
        .self_center()
        .px_3()
        .py_1()
        .text_size(px(spacing.small))
        .text_color(palette.text_muted)
        .child("Loading older messages…")
        .into_any_element()
}

fn day_separator(date: NaiveDate, palette: &Theme, spacing: Spacing) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .py_2()
        .child(rule(palette))
        .child(
            div()
                .text_size(px(spacing.small))
                .text_color(palette.text_muted)
                .child(SharedString::from(day_label(date))),
        )
        .child(rule(palette))
        .into_any_element()
}

fn unread_marker(palette: &Theme, spacing: Spacing) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .py_1()
        .child(div().h_px().flex_1().bg(palette.danger))
        .child(
            div()
                .text_size(px(spacing.small))
                .text_color(palette.danger)
                .child("Unread"),
        )
        .into_any_element()
}

fn update_line(message: &Message, palette: &Theme, spacing: Spacing) -> gpui::AnyElement {
    div()
        .self_center()
        .py_1()
        .text_size(px(spacing.small))
        .text_color(palette.text_muted)
        .child(SharedString::from(message.summary()))
        .into_any_element()
}

fn rule(palette: &Theme) -> gpui::Div {
    div().h_px().flex_1().bg(palette.border)
}

fn day_label(date: NaiveDate) -> String {
    let today = Local::now().date_naive();

    if date == today {
        "Today".into()
    } else if Some(date) == today.pred_opt() {
        "Yesterday".into()
    } else {
        date.format("%A, %-d %B %Y").to_string()
    }
}
