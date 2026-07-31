use gpui::prelude::*;
use gpui::{Context, Entity, SharedString, Window, div, px};
use uuid::Uuid;

use super::avatar::avatar;
use super::image;
use super::kit;
use crate::config::Theme;
use crate::data::attachment::{Blob, Kind};
use crate::data::{State, Thread};
use crate::store::{Focus, Store};
use crate::theme::ActivePalette;

/// Who or what the conversation is, and what has been shared in it.
pub struct Details {
    store: Entity<Store>,
}

impl Details {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
    }
}

impl Render for Details {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let store = self.store.read(cx);

        let Some(state) = store.state() else {
            return shell(&palette, div());
        };

        let body = match store.focus() {
            Some(Focus::Person(uuid)) => person(*uuid, state, &palette),
            None => match store.active() {
                Some(thread) => conversation(thread, state, &palette),
                None => div()
                    .p_4()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text_muted)
                    .child("Nothing selected."),
            },
        };

        shell(&palette, body)
    }
}

fn shell(palette: &Theme, body: gpui::Div) -> gpui::Stateful<gpui::Div> {
    div()
        .id("details")
        .size_full()
        .overflow_y_scroll()
        .bg(palette.surface)
        .text_color(palette.text)
        .child(body)
}

fn person(uuid: Uuid, state: &State, palette: &Theme) -> gpui::Div {
    let name = state.name_of(uuid);

    div()
        .flex()
        .flex_col()
        .child(hero(
            state.avatar_for(uuid).map(|path| path.to_path_buf()),
            &name,
            uuid.as_bytes(),
            palette,
        ))
        .child(fields(
            [
                ("Name", name.clone()),
                ("Account", uuid.to_string()),
                (
                    "You",
                    if uuid == state.aci { "Yes" } else { "No" }.to_string(),
                ),
            ],
            palette,
        ))
}

fn conversation(thread: &Thread, state: &State, palette: &Theme) -> gpui::Div {
    let name = state.title(thread);
    let kind = match thread {
        Thread::Contact(_) => "Direct message",
        Thread::Group(_) => "Group",
    };

    let shared: Vec<_> = state
        .history(thread)
        .map(|history| {
            history
                .messages()
                .iter()
                .rev()
                .flat_map(|message| message.attachments.iter())
                .filter(|attachment| matches!(attachment.kind, Kind::Image { .. }))
                .filter_map(|attachment| match &attachment.blob {
                    Blob::Cached(path) => Some(path.clone()),
                    _ => None,
                })
                .take(9)
                .collect()
        })
        .unwrap_or_default();

    div()
        .flex()
        .flex_col()
        .child(hero(
            state.avatar(thread).map(|path| path.to_path_buf()),
            &name,
            thread.seed(),
            palette,
        ))
        .child(fields([("Kind", kind.to_string())], palette))
        .when(!shared.is_empty(), |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .px_4()
                    .pb_4()
                    .child(kit::section("Shared media", palette))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1p5()
                            .children(shared.into_iter().map(|path| {
                                image::cropped(&path, 62.0)
                                    .rounded(px(kit::RADIUS))
                                    .into_any_element()
                            })),
                    ),
            )
        })
}

fn hero(
    picture: Option<std::path::PathBuf>,
    name: &str,
    seed: &[u8],
    palette: &Theme,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_2p5()
        .px_4()
        .pt_6()
        .pb_5()
        .child(avatar(picture.as_deref(), name, seed, 72.0, palette))
        .child(
            div()
                .text_size(px(palette.typography.ui_size + 3.0))
                .text_color(palette.text)
                .child(SharedString::from(name.to_owned())),
        )
}

fn fields<const N: usize>(rows: [(&'static str, String); N], palette: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .px_4()
        .pb_2()
        .children(rows.into_iter().map(|(label, value)| {
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap_3()
                .py_1p5()
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex_none()
                        .text_size(px(palette.typography.ui_size - 2.0))
                        .text_color(palette.text_muted)
                        .child(label),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(palette.typography.ui_size - 1.0))
                        .text_color(palette.text_dim)
                        .child(SharedString::from(value)),
                )
        }))
}
