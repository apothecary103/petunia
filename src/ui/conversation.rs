use chrono::{Local, NaiveDate};
use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, ScrollHandle, SharedString, Window, div, px};

use super::avatar::avatar;
use super::composer::Composer;
use super::kit;
use super::message::content;
use super::message::group::{self, Entry};
use super::relative;
use crate::config::Theme;
use crate::config::messages::{Spacing, Timestamps};
use crate::data::{Message, MessageId, State, Thread};
use crate::signal::Command;
use crate::store::{Focus, Store};
use crate::theme::ActivePalette;

/// Asks the worker for an attachment the auto-download policy skipped.
pub type Download =
    std::rc::Rc<dyn Fn(u64, &crate::data::attachment::Id, &mut Window, &mut gpui::App)>;

/// The focused conversation: its message list, and in the next phase the
/// composer beneath it.
pub struct Conversation {
    store: Entity<Store>,
    composer: Entity<Composer>,
    scroll: ScrollHandle,
    /// The thread the scroll position belongs to, so switching conversations
    /// starts at the newest message rather than wherever the last one was.
    anchored: Option<Thread>,
}

impl Conversation {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let composer = cx.new(|cx| Composer::new(store.clone(), window, cx));
        Self {
            store,
            composer,
            scroll: ScrollHandle::new(),
            anchored: None,
        }
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
            store.send(Command::LoadThread {
                thread,
                before: Some(before),
            });
            cx.notify();
        });
    }
}

impl Render for Conversation {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let store = self.store.read(cx);

        let Some(thread) = store.active().cloned() else {
            return empty(&palette, "Pick a conversation to start reading.").into_any_element();
        };
        let Some(state) = store.state() else {
            return empty(&palette, "Still connecting…").into_any_element();
        };

        let spacing = store.config.messages.density.spacing();
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

        let on_download: Download = {
            let store = self.store.clone();
            let thread = thread.clone();
            std::rc::Rc::new(move |timestamp, id, _window, cx| {
                store.update(cx, |store, cx| {
                    store.send(Command::DownloadAttachment {
                        thread: thread.clone(),
                        timestamp,
                        id: id.clone(),
                    });
                    cx.notify();
                });
            })
        };

        let entries = group::entries(history.messages(), history.first_unread(), group_within_ms);

        // A conversation opens at its newest message, and stays where it was
        // put once you have scrolled it yourself.
        if self.anchored.as_ref() != Some(&thread) {
            self.anchored = Some(thread.clone());
            self.scroll.scroll_to_bottom();
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
                Entry::Run(run) => {
                    let sender = run.sender;
                    let on_sender = std::rc::Rc::new(cx.listener(
                        move |this: &mut Self, _, _: &mut Window, cx: &mut Context<Self>| {
                            this.inspect(sender, cx)
                        },
                    ));
                    run_block(
                        &run,
                        state,
                        &palette,
                        spacing,
                        timestamps,
                        max_image,
                        on_sender,
                        on_download.clone(),
                    )
                    .into_any_element()
                }
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

fn run_block(
    run: &group::Run<'_>,
    state: &State,
    palette: &Theme,
    spacing: Spacing,
    timestamps: Timestamps,
    max_image: (f32, f32),
    on_sender: std::rc::Rc<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App)>,
    on_download: Download,
) -> gpui::Div {
    let name = state.sender_name(run.sender);
    let tint = palette.accent_for(run.sender.as_bytes());
    let first = run.messages.first();

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
                .on_mouse_down(MouseButton::Left, {
                    let on_sender = on_sender.clone();
                    move |event, window, cx| on_sender(event, window, cx)
                })
                .child(SharedString::from(name.clone())),
        )
        .when_some(first.filter(|_| timestamps != Timestamps::Never), |this, message| {
            this.child(
                div()
                    .text_size(px(spacing.small))
                    .text_color(palette.text_muted)
                    .child(SharedString::from(clock(message.timestamp()))),
            )
        });

    let bodies = run.messages.iter().map(|message| {
        content::Body {
            message,
            state,
            theme: palette,
            spacing,
            max_image,
            on_download: on_download.clone(),
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
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    on_sender(event, window, cx)
                })
                .child(avatar(
                    state.avatar_for(run.sender),
                    &name,
                    run.sender.as_bytes(),
                    spacing.avatar,
                    palette,
                )),
        )
        .child(
            div()
                .flex().flex_col()
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
