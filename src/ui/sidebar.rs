use std::path::Path;

use gpui::prelude::*;
use gpui::{Context, Div, Entity, MouseButton, SharedString, Stateful, Window, div, px};
use gpui_component::{ActiveTheme, StyledExt};

use super::avatar::avatar;
use crate::config::Theme;
use crate::data::{Section, Thread};
use crate::store::Store;
use crate::theme::ActivePalette;

/// The conversation list: quiet section headers, two-line entries, and the
/// account's own identity pinned to the bottom.
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
        let widgets = cx.theme();
        let store = self.store.read(cx);
        let active = store.active().cloned();

        let Some(state) = store.state() else {
            return shell(&palette, div());
        };

        let show_preview = store.config.sidebar.show_preview;
        let sections = [
            (Section::Pinned, "Pinned"),
            (Section::Requests, "Requests"),
            (Section::Chats, "Chats"),
            (Section::Archived, "Archived"),
        ];

        let mut list = div().v_flex().gap_4().px_2().pb_2();

        for (section, label) in sections {
            let entries: Vec<_> = state.index.section(section).collect();
            if entries.is_empty() {
                continue;
            }

            let rows = entries.into_iter().map(|entry| {
                let thread = entry.thread.clone();
                let selected = active.as_ref() == Some(&thread);
                let name = entry.name.clone();
                let preview = entry
                    .preview
                    .as_ref()
                    .filter(|_| show_preview)
                    .map(|message| message.summary());
                let unread = entry.unread;
                let mentions = entry.mentions;
                let seed = entry.thread.seed();
                let picture = state.avatar(&entry.thread).map(Path::to_path_buf);

                row(
                    &palette,
                    picture.as_deref(),
                    &name,
                    &seed,
                    preview.as_deref(),
                    unread,
                    mentions,
                    selected,
                )
                .id(SharedString::from(format!("thread-{seed:?}")))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.open(thread.clone(), cx)),
                )
            });

            list = list.child(
                div()
                    .v_flex()
                    .gap_px()
                    .child(header(label, &palette))
                    .children(rows),
            );
        }

        let body = div()
            .id("conversations")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(if state.index.is_empty() {
                div()
                    .p_4()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text_muted)
                    .child("Waiting for Signal to sync your conversations…")
                    .into_any_element()
            } else {
                list.into_any_element()
            });

        let footer = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(widgets.border)
            .child(avatar(
                state.avatar_for(state.aci),
                &state.own_name(),
                state.aci.as_bytes(),
                26.0,
                &palette,
            ))
            .child(
                div()
                    .v_flex()
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
                            .text_size(px(palette.typography.ui_size - 2.0))
                            .text_color(palette.text_muted)
                            .child(store.connection().label()),
                    ),
            );

        shell(&palette, div().v_flex().size_full().child(body).child(footer))
    }
}

fn shell(palette: &Theme, body: Div) -> Div {
    div()
        .size_full()
        .v_flex()
        .bg(palette.surface)
        .text_color(palette.text)
        .child(body)
}

fn header(label: &'static str, palette: &Theme) -> impl IntoElement {
    div()
        .px_2()
        .pt_2()
        .pb_1()
        .text_size(px(palette.typography.ui_size - 3.0))
        .text_color(palette.text_muted)
        .child(label)
}

#[expect(clippy::too_many_arguments)]
fn row(
    palette: &Theme,
    picture: Option<&Path>,
    name: &str,
    seed: &[u8],
    preview: Option<&str>,
    unread: u32,
    mentions: u32,
    selected: bool,
) -> Stateful<Div> {
    let title_color = if unread > 0 {
        palette.text
    } else if selected {
        palette.text
    } else {
        palette.text_dim
    };

    div()
        .id("row")
        .flex()
        .items_center()
        .gap_2p5()
        .px_2()
        .py_1p5()
        .rounded(px(6.0))
        .when(selected, |this| this.bg(palette.selected))
        .when(!selected, |this| this.hover(|this| this.bg(palette.hover)))
        .child(avatar(picture, name, seed, 28.0, palette))
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_px()
                .child(
                    div()
                        .text_size(px(palette.typography.ui_size))
                        .text_color(title_color)
                        .truncate()
                        .child(SharedString::from(name.to_owned())),
                )
                .when_some(preview, |this, preview| {
                    this.child(
                        div()
                            .text_size(px(palette.typography.ui_size - 2.0))
                            .text_color(palette.text_muted)
                            .truncate()
                            .child(SharedString::from(preview.to_owned())),
                    )
                }),
        )
        .when(unread > 0, |this| {
            this.child(badge(unread, mentions, palette))
        })
}


fn badge(unread: u32, mentions: u32, palette: &Theme) -> impl IntoElement {
    let mentioned = mentions > 0;

    div()
        .px_1p5()
        .py_0p5()
        .rounded_full()
        .bg(if mentioned {
            palette.accent
        } else {
            palette.active
        })
        .text_size(px(palette.typography.ui_size - 3.0))
        .text_color(if mentioned {
            palette.on_accent
        } else {
            palette.text_dim
        })
        .child(SharedString::from(unread.to_string()))
}

