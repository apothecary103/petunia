//! Sending a message on somewhere else: type a few letters, pick where.
//!
//! The switcher's shape, because the question is the same one -- which
//! conversation -- and answering it a second, different way would be a second
//! thing to learn.

use gpui::prelude::*;
use gpui::{
    Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, SharedString, Window, div,
    px,
};

use super::avatar::avatar;
use super::kit;
use super::switcher;
use petunia_data::{MessageId, Thread};
use crate::store::Store;
use crate::theme::ActivePalette;

pub struct Dismissed;

/// Where it is to go. The workspace does the sending, because the message is
/// the store's and not this picker's.
#[derive(Debug, Clone)]
pub struct Picked {
    pub target: MessageId,
    pub thread: Thread,
}

impl gpui::EventEmitter<Dismissed> for Forward {}
impl gpui::EventEmitter<Picked> for Forward {}

pub struct Forward {
    store: Entity<Store>,
    /// What is being forwarded, carried through so the picker answers with both
    /// halves and the workspace needs no memory of its own.
    target: MessageId,
    /// A one-line summary of it, so you can see what you are about to send.
    summary: String,
    focus: FocusHandle,
    query: String,
    selected: usize,
    /// So the arrow keys can bring the selection back into view.
    scroll: gpui::ScrollHandle,
}

impl Forward {
    pub fn new(
        store: Entity<Store>,
        target: MessageId,
        summary: String,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self {
            store,
            target,
            summary,
            focus: cx.focus_handle(),
            query: String::new(),
            selected: 0,
            scroll: gpui::ScrollHandle::new(),
        }
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn hits(&self, cx: &Context<Self>) -> Vec<(Thread, String)> {
        self.store
            .read(cx)
            .state()
            .map(|state| {
                switcher::matches(&state.index, &self.query)
                    .into_iter()
                    .map(|entry| (entry.thread.clone(), entry.name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        let keystroke = &event.keystroke;

        match keystroke.key.as_str() {
            "escape" => cx.emit(Dismissed),
            "enter" => {
                if let Some((thread, _)) = hits.get(self.selected) {
                    self.send_to(thread.clone(), cx);
                }
            }
            "down" => self.move_by(1, hits.len(), cx),
            "up" => self.move_by(-1, hits.len(), cx),
            "backspace" => {
                self.query.pop();
                self.selected = 0;
                cx.notify();
            }
            _ => {
                if let Some(typed) = keystroke.key_char.as_ref().filter(|typed| {
                    !typed.is_empty()
                        && !keystroke.modifiers.platform
                        && !keystroke.modifiers.control
                }) {
                    self.query.push_str(typed);
                    self.selected = 0;
                    cx.notify();
                }
            }
        }
    }

    fn move_by(&mut self, delta: isize, len: usize, cx: &mut Context<Self>) {
        self.selected = switcher::step(self.selected, delta, len);
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    fn send_to(&mut self, thread: Thread, cx: &mut Context<Self>) {
        cx.emit(Picked {
            target: self.target,
            thread,
        });
        cx.emit(Dismissed);
    }
}

impl Focusable for Forward {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Forward {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let hits = self.hits(cx);
        let pictures: Vec<_> = self
            .store
            .read(cx)
            .state()
            .map(|state| {
                hits.iter()
                    .map(|(thread, _)| state.avatar(thread).map(|path| path.to_path_buf()))
                    .collect()
            })
            .unwrap_or_default();

        let rows = hits.iter().enumerate().map(|(at, (thread, name))| {
            let picture = pictures.get(at).cloned().flatten();
            let target = thread.clone();

            kit::row(
                SharedString::from(format!("to-{at}")),
                at == self.selected,
                &palette,
            )
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.send_to(target.clone(), cx)),
            )
            .child(avatar(
                picture.as_deref(),
                name,
                thread.seed(),
                24.0,
                &palette,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text)
                    .child(SharedString::from(name.clone())),
            )
        });

        div()
            .id("forward-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .items_center()
            .bg(gpui::Hsla {
                a: 0.6,
                ..palette.background
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key))
                    .mt(px(96.0))
                    .w(px(520.0))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .rounded(px(kit::RADIUS_LG))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.heading(&palette))
                    .child(
                        div()
                            .id("candidates")
                            // Bounded and scrolling, so a list longer than the
                            // window is one the keyboard can still walk.
                            .max_h(px(420.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .p_1p5()
                            .children(rows)
                            .when(hits.is_empty(), |this| {
                                this.child(
                                    div()
                                        .px_2()
                                        .py_3()
                                        .text_size(px(palette.typography.ui_size))
                                        .text_color(palette.text_muted)
                                        .child("No conversations match."),
                                )
                            }),
                    ),
            )
    }
}

impl Forward {
    /// What is being sent, and what has been typed to find where to send it.
    fn heading(&self, palette: &petunia_config::Theme) -> impl IntoElement {
        let empty = self.query.is_empty();

        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3p5()
            .py_3()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(kit::icon(
                        gpui_component::IconName::Redo,
                        14.0,
                        palette.text_muted,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(palette.typography.message_size))
                            .text_color(match empty {
                                true => palette.text_muted,
                                false => palette.text,
                            })
                            .child(SharedString::from(match empty {
                                true => "Forward to…".to_owned(),
                                false => self.query.clone(),
                            })),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(palette.typography.ui_size - 1.0))
                    .text_color(palette.text_dim)
                    .child(SharedString::from(self.summary.clone())),
            )
    }
}
