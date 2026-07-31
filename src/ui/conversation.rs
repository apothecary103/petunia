use chrono::{Local, NaiveDate};
use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, ScrollHandle, SharedString, Window, div, px};

use super::avatar::avatar;
use super::composer::Composer;
use super::kit;
use super::message::act::{Act, Dispatch};
use super::message::content;
use super::message::group::{self, Entry};
use super::relative;
use crate::audio::{Playback, Player};
use crate::config::Theme;
use crate::config::messages::{Spacing, Timestamps};
use crate::data::{Message, MessageId, State, Thread};
use crate::signal::Command;
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
    scroll: ScrollHandle,
    /// The thread the scroll position belongs to, so switching conversations
    /// starts at the newest message rather than wherever the last one was.
    anchored: Option<Thread>,
    /// How many messages were on screen last frame. A new one arriving should
    /// bring the view with it -- but only if you were already at the bottom, or
    /// reading back through history would yank you away mid-sentence.
    counted: usize,
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
            |this: &mut Self, _, event: &crate::store::StoreEvent, window, cx| {
                // A page of *older* messages is inserted above what is on
                // screen, which would otherwise shove the reader down by exactly
                // the height of what arrived. How tall that is is only knowable
                // once it has been laid out, so the correction waits a frame.
                if let crate::store::StoreEvent::History { older: true } = event {
                    let scroll = this.scroll.clone();
                    let before = scroll.max_offset().y;
                    window.on_next_frame(move |_, _| {
                        let grown = scroll.max_offset().y - before;
                        let mut offset = scroll.offset();
                        offset.y -= grown;
                        scroll.set_offset(offset);
                    });
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
            scroll: ScrollHandle::new(),
            anchored: None,
            counted: 0,
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
                        store.state().is_some_and(crate::data::State::anyone_typing)
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

    /// Whether the list is close enough to the end to count as being at it.
    /// A threshold rather than an equality: a fractional offset is normal, and
    /// one pixel of slack should not mean "the reader has scrolled away".
    fn at_bottom(&self) -> bool {
        const SLACK: f32 = 96.0;

        let remaining = f32::from(self.scroll.max_offset().y) + f32::from(self.scroll.offset().y);
        remaining.abs() <= SLACK
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
                .is_some_and(crate::data::History::is_loading)
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

        let entries = group::entries(history.messages(), history.first_unread(), group_within_ms);

        // A conversation opens at its newest message, and stays where it was
        // put once you have scrolled it yourself.
        let count = history.messages().len();
        if self.anchored.as_ref() != Some(&thread) {
            self.anchored = Some(thread.clone());
            self.counted = count;
            self.scroll.scroll_to_bottom();
            // A voice note belongs to the conversation it was sent in, so it
            // does not follow you into the next one.
            self.player.stop();
        } else if count > self.counted {
            // Measured before the new message is laid out, which is the whole
            // point: "were you at the bottom a moment ago", not "are you at the
            // bottom now that something has been added below you".
            let follow = self.at_bottom();
            self.counted = count;

            if follow {
                let scroll = self.scroll.clone();
                window.on_next_frame(move |_, _| scroll.scroll_to_bottom());
            }
        } else {
            self.counted = count;
        }

        let mut list = kit::measured()
            .flex()
            .flex_col()
            .px(px(spacing.padding_x))
            .py(px(spacing.padding_y))
            .gap(px(spacing.between_runs));

        if history.has_more() {
            let loading = history.is_loading();
            let target = thread.clone();
            list = list.child(
                div()
                    .id("load-older")
                    .self_center()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .text_size(px(spacing.small))
                    .text_color(palette.text_muted)
                    .when(!loading, |this| {
                        this.hover(|this| this.bg(palette.hover)).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.load_older(target.clone(), cx)
                            }),
                        )
                    })
                    .child(if loading {
                        "Loading older messages…"
                    } else {
                        "Load older messages"
                    }),
            );
        }

        for entry in entries {
            list = list.child(match entry {
                Entry::Day(date) => day_separator(date, &palette, spacing),
                Entry::UnreadMarker => unread_marker(&palette, spacing),
                Entry::Update(message) => update_line(message, &palette, spacing),
                Entry::Run(run) => run_block(
                    &run,
                    Run {
                        state,
                        palette: &palette,
                        spacing,
                        timestamps,
                        max_image,
                        playback: &playback,
                        act: &act,
                    },
                )
                .into_any_element(),
            });
        }

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
                div()
                    .id("messages")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        // A short thread sits at the bottom of the window, not
                        // stranded at the top of an empty one.
                        div()
                            .flex()
                            .flex_col()
                            .justify_end()
                            .min_h_full()
                            .w_full()
                            .child(list),
                    ),
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

/// Everything a run of messages needs that is not the messages themselves.
struct Run<'a> {
    state: &'a State,
    palette: &'a Theme,
    spacing: Spacing,
    timestamps: Timestamps,
    max_image: (f32, f32),
    playback: &'a Playback,
    act: &'a Dispatch,
}

fn run_block(run: &group::Run<'_>, frame: Run<'_>) -> gpui::Div {
    let Run {
        state,
        palette,
        spacing,
        timestamps,
        max_image,
        playback,
        act,
    } = frame;

    let sender = run.sender;
    let name = state.sender_name(sender);
    let tint = palette.accent_for(sender.as_bytes());
    let first = run.messages.first();
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
                .text_color(tint)
                .hover(|this| this.underline())
                .on_mouse_down(MouseButton::Left, inspect.clone())
                .on_mouse_down(MouseButton::Right, menu.clone())
                .child(SharedString::from(name.clone())),
        )
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

    let bodies = run.messages.iter().map(|message| {
        content::Body {
            message,
            state,
            theme: palette,
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
