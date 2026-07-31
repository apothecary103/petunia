use std::path::Path;

use gpui::prelude::*;
use gpui::{Context, Div, Entity, MouseButton, SharedString, Stateful, Window, div, px};

use super::avatar::avatar;
use super::kit;
use super::relative;
use crate::config::Theme;
use crate::data::{Section, Thread};
use crate::store::Store;
use crate::theme::ActivePalette;

/// The conversation list: quiet section headers, two-line entries carrying the
/// name, time and preview, and the account's identity pinned to the bottom.
pub struct Sidebar {
    store: Entity<Store>,
}

impl Sidebar {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
    }

    fn open(&mut self, thread: Thread, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.activate(thread, cx));
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let store = self.store.read(cx);
        let active = store.active().cloned();

        let Some(state) = store.state() else {
            return shell(&palette, div());
        };

        let show_preview = store.config.sidebar.show_preview;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let sections = state.index.sections();

        let mut list = div().flex().flex_col().gap_4().px_2p5().pb_3();

        for section in sections {
            let label = match &section {
                Section::Pinned => "Pinned".to_string(),
                Section::Requests => "Requests".to_string(),
                Section::Chats => "Chats".to_string(),
                Section::Archived => "Archived".to_string(),
                Section::Folder(name) => name.clone(),
            };
            // A contact sync lists everyone you have ever known; only threads
            // with something in them belong in a conversation list.
            let entries: Vec<_> = state
                .index
                .section(section.clone())
                .filter(|entry| entry.started())
                .collect();
            if entries.is_empty() {
                continue;
            }

            let rows = entries.into_iter().map(|entry| {
                let thread = entry.thread.clone();
                let selected = active.as_ref() == Some(&thread);
                let seed = entry.thread.seed().to_vec();

                let wanted = thread.clone();

                row(
                    &palette,
                    Line {
                        picture: state.avatar(&entry.thread),
                        name: &entry.name,
                        seed: &seed,
                        preview: entry
                            .preview
                            .as_ref()
                            .filter(|_| show_preview)
                            .map(|message| message.summary()),
                        when: entry.last_activity,
                        unread: entry.unread,
                        mentions: entry.mentions,
                        muted: entry.flags.muted(now),
                        pinned: entry.flags.pinned,
                        selected,
                    },
                )
                .id(SharedString::from(format!("thread-{}", hex(&seed))))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.open(thread.clone(), cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this: &mut Self, event: &gpui::MouseDownEvent, _, cx| {
                        let thread = wanted.clone();
                        this.store
                            .update(cx, |store, cx| store.open_menu(thread, event.position, cx));
                    }),
                )
            });

            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(kit::section(label, &palette))
                    .children(rows),
            );
        }

        let body = div()
            .id("conversations")
            .flex_1()
            .min_h_0()
            // The window's traffic lights float over the top of this column.
            .pt(px(super::workspace::TITLE_BAR))
            .overflow_y_scroll()
            .child(if state.index.is_empty() {
                div()
                    .px_4()
                    .py_6()
                    .text_size(px(palette.typography.ui_size - 1.0))
                    .text_color(palette.text_muted)
                    .child("Waiting for Signal to sync your conversations…")
                    .into_any_element()
            } else {
                list.into_any_element()
            });

        shell(
            &palette,
            div()
                .flex().flex_col()
                .size_full()
                .child(body)
                .child(identity(state, &palette)),
        )
    }
}

fn identity(state: &crate::data::State, palette: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2p5()
        .px_3p5()
        .py_3()
        .child(avatar(
            state.avatar_for(state.aci),
            &state.own_name(),
            state.aci.as_bytes(),
            30.0,
            palette,
        ))
        .child(
            div()
                .flex().flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(palette.typography.ui_size))
                        .text_color(palette.text)
                        .truncate()
                        .child(SharedString::from(state.own_name())),
                )
                .child(
                    div()
                        .text_size(px(palette.typography.ui_size - 3.0))
                        .text_color(palette.text_muted)
                        .child(state.connection.label()),
                ),
        )
}

fn shell(palette: &Theme, body: Div) -> Div {
    div()
        .size_full()
        .flex().flex_col()
        .bg(palette.surface)
        .text_color(palette.text)
        .child(body)
}

struct Line<'a> {
    picture: Option<&'a Path>,
    name: &'a str,
    seed: &'a [u8],
    preview: Option<String>,
    when: u64,
    unread: u32,
    mentions: u32,
    muted: bool,
    pinned: bool,
    selected: bool,
}

fn row(palette: &Theme, line: Line<'_>) -> Stateful<Div> {
    let unread = line.unread > 0;
    let title_color = if line.muted {
        palette.text_dim
    } else if unread || line.selected {
        palette.text
    } else {
        palette.text_dim
    };

    kit::row("row", line.selected, palette)
        .child(
            div()
                .pt_0p5()
                .child(avatar(line.picture, line.name, line.seed, 30.0, palette)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(palette.typography.ui_size))
                                .text_color(title_color)
                                .child(SharedString::from(line.name.to_owned())),
                        )
                        .when(line.pinned, |this| {
                            this.child(kit::icon(
                                gpui_component::IconName::Star,
                                10.0,
                                palette.text_muted,
                            ))
                        })
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(palette.typography.ui_size - 3.0))
                                .text_color(palette.text_muted)
                                .child(SharedString::from(relative::short(line.when))),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(palette.typography.ui_size - 2.0))
                                .text_color(if unread {
                                    palette.text_dim
                                } else {
                                    palette.text_muted
                                })
                                .child(SharedString::from(line.preview.unwrap_or_default())),
                        )
                        .when(unread, |this| {
                            this.child(badge(line.unread, line.mentions, line.muted, palette))
                        }),
                ),
        )
}

/// A dot when there is simply something new, a count once it is worth counting,
/// and the accent reserved for a mention. A muted conversation still counts, but
/// dimly: Signal's own behaviour, and the difference between "there is something
/// here" and "look at this".
fn badge(unread: u32, mentions: u32, muted: bool, palette: &Theme) -> Div {
    let mentioned = mentions > 0 && !muted;
    let tint = if mentioned {
        palette.accent
    } else if muted {
        palette.text_muted
    } else {
        palette.text_dim
    };

    if unread == 1 && !mentioned {
        return kit::dot(tint);
    }
    kit::chip(unread.to_string(), tint, palette)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
