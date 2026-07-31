use chrono::{DateTime, Local, NaiveDate};
use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, SharedString, Window, div, px};
use gpui_component::StyledExt;

use super::avatar::avatar;
use super::message::group::{self, Entry};
use crate::config::Theme;
use crate::config::messages::{Spacing, Timestamps};
use crate::data::{Message, State, Thread};
use crate::signal::Command;
use crate::store::Store;
use crate::theme::ActivePalette;

/// The focused conversation: its message list, and in the next phase the
/// composer beneath it.
pub struct Conversation {
    store: Entity<Store>,
}

impl Conversation {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
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
            return empty(&palette, "Pick a conversation to start reading.");
        };
        let Some(state) = store.state() else {
            return empty(&palette, "Still connecting…");
        };

        let spacing = store.config.messages.density.spacing();
        let timestamps = store.config.messages.timestamps;
        let group_within = store.config.messages.group_within;

        let Some(history) = state.history(&thread) else {
            return empty(&palette, "Loading…");
        };
        if history.is_empty() {
            return empty(&palette, "No messages here yet.");
        }

        let entries = group::entries(history.messages(), history.first_unread(), group_within);

        let mut list = div()
            .v_flex()
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
                    run_block(&run, state, &palette, spacing, timestamps).into_any_element()
                }
            });
        }

        div()
            .size_full()
            .bg(palette.background)
            .child(
                div()
                    .id("messages")
                    .size_full()
                    .overflow_y_scroll()
                    .child(list),
            )
    }
}

fn empty(palette: &Theme, message: &'static str) -> gpui::Div {
    div()
        .size_full()
        .v_flex()
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
                .text_size(px(spacing.body))
                .text_color(tint)
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
        div()
            .text_size(px(spacing.body))
            .text_color(palette.text)
            .child(SharedString::from(message.summary()))
    });

    div()
        .flex()
        .items_start()
        .gap(px(spacing.gutter - spacing.avatar))
        .child(avatar(
            state.avatar_for(run.sender),
            &name,
            run.sender.as_bytes(),
            spacing.avatar,
            palette,
        ))
        .child(
            div()
                .v_flex()
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
    local(timestamp)
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

fn local(timestamp: u64) -> Option<DateTime<Local>> {
    DateTime::from_timestamp_millis(timestamp as i64).map(|at| at.with_timezone(&Local))
}
