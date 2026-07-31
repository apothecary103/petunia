//! Asking before something cannot be undone.
//!
//! Petunia's own rather than the platform's alert: an `NSAlert` is the system's
//! chrome in the middle of a window that is otherwise entirely petunia's, and it
//! cannot be themed. This is `ui::prompt` with the text field taken out and the
//! confirming button in the warning colour.

use gpui::prelude::*;
use gpui::{Context, MouseButton, SharedString, Window, div, px};

use super::kit;
use crate::theme::ActivePalette;

pub struct Dismissed;

/// The dialog was confirmed. Carries nothing: what it was about is whatever the
/// caller was holding when it asked.
pub struct Confirmed;

impl gpui::EventEmitter<Dismissed> for Confirm {}
impl gpui::EventEmitter<Confirmed> for Confirm {}

pub struct Confirm {
    title: SharedString,
    /// What will actually happen, in a sentence. A dialog that says only
    /// "are you sure?" leaves the reader to guess at the consequence.
    detail: SharedString,
    confirm: SharedString,
    focus: gpui::FocusHandle,
}

impl Confirm {
    pub fn new(
        title: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        confirm: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            confirm: confirm.into(),
            focus: cx.focus_handle(),
        }
    }

    /// Focused so that Escape reaches it: actions dispatch along the focus path,
    /// and a dialog nothing has focused could not be cancelled by the keyboard.
    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }
}

impl gpui::Focusable for Confirm {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Confirm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();

        kit::scrim(&palette)
            .id("confirm")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                kit::dialog(380.0, &palette)
                    .child(
                        div()
                            .text_size(px(palette.typography.ui_size + 1.0))
                            .text_color(palette.text)
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(palette.typography.ui_size - 1.0))
                            .line_height(px((palette.typography.ui_size - 1.0) * 1.5))
                            .text_color(palette.text_muted)
                            .child(self.detail.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(kit::button(
                                "cancel",
                                "Cancel",
                                kit::Intent::Quiet,
                                &palette,
                                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
                            ))
                            .child(kit::button(
                                "confirm",
                                self.confirm.clone(),
                                kit::Intent::Danger,
                                &palette,
                                cx.listener(|_: &mut Self, _, _, cx| {
                                    cx.emit(Confirmed);
                                    cx.emit(Dismissed);
                                }),
                            )),
                    ),
            )
    }
}
