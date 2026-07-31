use gpui::prelude::*;
use gpui::{
    Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, SharedString, Window, div,
    px,
};

use super::avatar::avatar;
use super::kit;
use super::switcher;
use petunia_config::Theme;
use petunia_data::Thread;
use crate::store::Store;
use crate::theme::ActivePalette;

/// The quick switcher: type a few letters, jump to a conversation. Its own
/// entity so the query and the highlighted row survive a repaint.
pub struct Switcher {
    store: Entity<Store>,
    focus: FocusHandle,
    query: String,
    selected: usize,
    /// So the arrow keys can bring the selection back into view. A list walked by
    /// keyboard that scrolls only under the pointer selects rows you cannot see.
    scroll: gpui::ScrollHandle,
}

impl Switcher {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self {
            store,
            focus: cx.focus_handle(),
            query: String::new(),
            selected: 0,
            scroll: gpui::ScrollHandle::new(),
        }
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.selected = 0;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The threads the current query selects, resolved the same way the view
    /// draws them so the highlighted row and the opened one always agree.
    fn hits(&self, cx: &Context<Self>) -> Vec<(Thread, String, u32)> {
        self.store
            .read(cx)
            .state()
            .map(|state| {
                switcher::matches(&state.index, &self.query)
                    .into_iter()
                    .map(|entry| (entry.thread.clone(), entry.name.clone(), entry.unread))
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
                if let Some((thread, _, _)) = hits.get(self.selected) {
                    let thread = thread.clone();
                    self.store
                        .update(cx, |store, cx| store.activate(thread, cx));
                    cx.emit(Dismissed);
                }
            }
            "down" => self.move_by(1, hits.len(), cx),
            "up" => self.move_by(-1, hits.len(), cx),
            // cmd+j walks the list too, matching the binding that cycles unread.
            "j" if keystroke.modifiers.platform => self.move_by(1, hits.len(), cx),
            "k" if keystroke.modifiers.platform => self.move_by(-1, hits.len(), cx),
            "backspace" => {
                self.query.pop();
                self.selected = 0;
                cx.notify();
            }
            _ => {
                // Whatever the keyboard actually produced, so accented and
                // non-latin input types the same as ascii.
                if let Some(typed) = keystroke.key_char.as_ref().filter(|typed| {
                    !typed.is_empty() && !keystroke.modifiers.platform && !keystroke.modifiers.control
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

    fn open(&mut self, thread: Thread, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.activate(thread, cx));
        cx.emit(Dismissed);
    }
}

/// Raised when the switcher is finished with, so the workspace can put it away.
pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for Switcher {}

impl Focusable for Switcher {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Switcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let hits = self.hits(cx);
        let avatars: Vec<_> = self
            .store
            .read(cx)
            .state()
            .map(|state| {
                hits.iter()
                    .map(|(thread, _, _)| state.avatar(thread).map(|path| path.to_path_buf()))
                    .collect()
            })
            .unwrap_or_default();

        let rows = hits.iter().enumerate().map(|(at, (thread, name, unread))| {
            let picture = avatars.get(at).cloned().flatten();
            let target = thread.clone();

            kit::row(SharedString::from(format!("hit-{at}")), at == self.selected, &palette)
                .items_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.open(target.clone(), cx)),
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
                .when(*unread > 0, |this| {
                    this.child(kit::chip(unread.to_string(), palette.text_dim, &palette))
                })
        });

        // A scrim, so the rest of the window reads as out of reach while this
        // is up, and a click anywhere on it puts the switcher away.
        div()
            .id("switcher-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .items_center()
            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.45))
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.emit(Dismissed)))
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
                    // Clicks inside must not reach the scrim behind it.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(query_line(&self.query, &palette))
                    .child(
                        div()
                            .id("matches")
                            // Bounded and scrolling: a query that matches
                            // everything grew the sheet past the bottom of the
                            // window, which put the rows nobody could see in the
                            // one place the keyboard was walking to.
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

fn query_line(query: &str, palette: &Theme) -> impl IntoElement {
    let empty = query.is_empty();

    div()
        .flex()
        .items_center()
        .gap_2p5()
        .px_3p5()
        .py_3()
        .border_b_1()
        .border_color(palette.border)
        .child(kit::icon(
            gpui_component::IconName::Search,
            15.0,
            palette.text_muted,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(palette.typography.message_size))
                .text_color(if empty {
                    palette.text_muted
                } else {
                    palette.text
                })
                .child(SharedString::from(if empty {
                    "Jump to a conversation…".to_owned()
                } else {
                    query.to_owned()
                })),
        )
}
