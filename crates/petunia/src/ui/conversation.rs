use chrono::{Local, NaiveDate};
use gpui::prelude::*;
use gpui::{
    Context, Entity, Hsla, ListAlignment, ListState, MouseButton, SharedString, Window, div, list,
    px,
};
use uuid::Uuid;

use super::avatar::avatar;
use super::composer::Composer;
use super::kit;
use super::message::act::{Act, Dispatch};
use super::message::content;
use super::message::group;
use super::relative;
use petunia_media::audio::{Playback, Player};
use petunia_config::Theme;
use petunia_config::messages::{Spacing, Timestamps};
use petunia_data::{Member, Message, MessageId, State, Thread};
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

/// How far beyond the window the list measures. A run is tall and its height is
/// only known once it is built, so a scroll that outruns the measured region
/// stutters while it catches up; a window's worth of slack is enough that a
/// flick never does.
const OVERDRAW: f32 = 800.0;

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
    /// How many rows the list was last told about, and whether the next change
    /// arrived above what is on screen. The list addresses its scroll position as
    /// an item and an offset into it, so rows spliced in at the front carry the
    /// reader with them -- which is why loading older messages no longer needs to
    /// measure how far it shoved them.
    counted: usize,
    prepending: bool,
    /// Runs only while something is playing, because a repaint every tenth of a
    /// second for an idle window is not free.
    ticking: Option<gpui::Task<()>>,
    /// Ages out typing indicators. `State::typing` already filters by elapsed
    /// time, but nothing would ask it again, so an indicator whose "stopped" was
    /// lost would sit there until the next unrelated repaint.
    aging: Option<gpui::Task<()>>,
}

impl Conversation {
    pub fn new(
        store: Entity<Store>,
        player: Player,
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
                if let crate::store::StoreEvent::History { older: true } = event {
                    this.prepending = true;
                    cx.notify();
                }
            },
        )
        .detach();
        let composer = cx.new(|cx| Composer::new(store.clone(), window, cx));
        Self {
            store,
            composer,
            player,
            // Bottom-anchored, like every chat log: with nothing scrolled it sits
            // at the newest message and stays there as messages arrive.
            list: ListState::new(0, ListAlignment::Bottom, px(OVERDRAW)),
            anchored: None,
            counted: 0,
            prepending: false,
            ticking: None,
            aging: None,
        }
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
            Act::Delete(target) => self
                .store
                .update(cx, |store, cx| store.delete(thread, target, cx)),
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
            Act::InstallStickers { pack_id, key } => self.store.update(cx, |store, _| {
                store.send(Command::InstallStickerPack { pack_id, key })
            }),
            Act::Inspect(who) => self.inspect(who, cx),
            Act::Menu(target, at) => self.open_menu(target, at, cx),
            Act::MenuFor(who, at) => {
                let items = crate::ui::menu::message::person(who, &self.dispatch(cx));
                cx.emit(Raise::new(items, at));
            }
        }
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

    fn copy(&self, target: MessageId, cx: &mut Context<Self>) {
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

    /// Tells the list how many rows there are and, when that has changed, which
    /// end they arrived at.
    ///
    /// The distinction is the whole point: rows spliced in at the front carry the
    /// reader down with them, so what they were reading stays under the pointer,
    /// while rows appended at the back leave them where they are -- and leave a
    /// reader who was at the bottom at the bottom, because that is what a
    /// bottom-anchored list does with no scroll position of its own.
    fn reconcile(&mut self, thread: &Thread, count: usize) {
        if self.anchored.as_ref() != Some(thread) {
            self.anchored = Some(thread.clone());
            self.counted = count;
            self.prepending = false;
            self.list.reset(count);
            // A voice note belongs to the conversation it was sent in, so it
            // does not follow you into the next one.
            self.player.stop();
            return;
        }

        let was = std::mem::replace(&mut self.counted, count);
        let prepending = std::mem::take(&mut self.prepending);

        match count.checked_sub(was) {
            None => self.list.reset(count),
            Some(0) => {}
            Some(added) if prepending => self.list.splice(0..0, added),
            Some(added) => self.list.splice(was..was, added),
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

        self.reconcile(&thread, rows.len() + 1);

        let rows = std::rc::Rc::new(rows);
        let frame = std::rc::Rc::new(Frame {
            thread: thread.clone(),
            palette: palette.clone(),
            highlights: cx.highlights().clone(),
            spacing,
            timestamps,
            max_image,
            playback,
            act,
        });
        let store = self.store.clone();
        let target = thread.clone();
        let this = cx.entity();

        let list = list(self.list.clone(), move |index, _window, cx| {
            let Some(row) = index.checked_sub(1) else {
                return match older {
                    true => load_older(loading, &frame.palette, spacing, {
                        let this = this.clone();
                        let target = target.clone();
                        move |_: &gpui::MouseDownEvent, _: &mut Window, cx: &mut gpui::App| {
                            this.update(cx, |this, cx| this.load_older(target.clone(), cx));
                        }
                    }),
                    false => div().into_any_element(),
                };
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
                kit::measured()
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
    spacing: Spacing,
    timestamps: Timestamps,
    max_image: (f32, f32),
    playback: Playback,
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
                run_block(*sender, run, state, self).into_any_element()
            }
        })
    }
}

/// The button that fetches the previous page, which is the top of the list when
/// there is more behind it.
fn load_older(
    loading: bool,
    palette: &Theme,
    spacing: Spacing,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    div()
        .id("load-older")
        .self_center()
        .px_3()
        .py_1()
        .rounded_full()
        .text_size(px(spacing.small))
        .text_color(palette.text_muted)
        .when(!loading, |this| {
            this.hover(|this| this.bg(palette.hover))
                .on_mouse_down(MouseButton::Left, on_click)
        })
        .child(if loading {
            "Loading older messages…"
        } else {
            "Load older messages"
        })
        .into_any_element()
}

fn run_block(
    sender: Uuid,
    run: &[Message],
    state: &State,
    frame: &Frame,
) -> gpui::Div {
    let Frame {
        thread,
        palette,
        highlights,
        spacing,
        timestamps,
        max_image,
        playback,
        act,
    } = frame;
    let (spacing, timestamps, max_image) = (*spacing, *timestamps, *max_image);
    let name = state.sender_name(sender);
    let member = state.member(thread, sender);
    let tint = palette.accent_for(sender.as_bytes());
    let first = run.first();
    let inspect = {
        let act = act.clone();
        move |_: &gpui::MouseDownEvent, window: &mut Window, cx: &mut gpui::App| {
            act(Act::Inspect(sender), window, cx)
        }
    };
    let menu = {
        let act = act.clone();
        move |event: &gpui::MouseDownEvent, window: &mut Window, cx: &mut gpui::App| {
            act(Act::MenuFor(sender, event.position), window, cx)
        }
    };

    let header = div()
        .flex()
        .items_baseline()
        .gap_2()
        .child(
            div()
                .id("sender")
                .cursor_pointer()
                .text_size(px(spacing.body))
                .font_weight(kit::EMPHASIS)
                .text_color(tint)
                .hover(|this| this.underline())
                .on_mouse_down(MouseButton::Left, inspect.clone())
                .on_mouse_down(MouseButton::Right, menu.clone())
                .child(SharedString::from(name.clone())),
        )
        .children(labels(member, tint, palette, spacing))
        .when_some(
            first.filter(|_| timestamps != Timestamps::Never),
            |this, message| {
                this.child(
                    div()
                        .text_size(px(spacing.small))
                        .text_color(palette.text_muted)
                        .child(SharedString::from(clock(message.timestamp()))),
                )
            },
        );

    let bodies = run.iter().map(|message| {
        content::Body {
            message,
            state,
            theme: palette,
            highlights,
            spacing,
            max_image,
            playback,
            act,
        }
        .render()
    });

    div()
        .flex()
        .items_start()
        .gap(px(spacing.gutter - spacing.avatar))
        .child(
            div()
                .id("sender-avatar")
                .flex_none()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, inspect)
                .on_mouse_down(MouseButton::Right, menu)
                .child(avatar(
                    state.avatar_for(sender),
                    &name,
                    sender.as_bytes(),
                    spacing.avatar,
                    palette,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(spacing.within_run))
                .child(header)
                .children(bodies),
        )
}

/// What the group says about whoever is talking: the label they picked for
/// themselves, then the role the group gave them. Empty outside a group, and
/// empty for the ordinary members of one, so a chip here always means something.
/// The chips beside a run's header. `tint` is the sender's own colour, because a
/// label someone picked for themselves belongs to them the way their name does;
/// the role is the group's word rather than theirs, and stays neutral.
fn labels(member: Option<&Member>, tint: Hsla, palette: &Theme, spacing: Spacing) -> Vec<gpui::Div> {
    let Some(member) = member else {
        return Vec::new();
    };
    let size = spacing.small - 1.0;

    member
        .badge()
        .map(|badge| kit::chip_sized(badge, tint, size))
        .into_iter()
        .chain(
            member
                .role
                .label()
                .map(|role| kit::chip_sized(role, palette.text_dim, size)),
        )
        .collect()
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

fn clock(timestamp: u64) -> String {
    relative::local(timestamp)
        .map(|at| at.format("%H:%M").to_string())
        .unwrap_or_default()
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
