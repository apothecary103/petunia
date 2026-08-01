//! The keybinding sheet, generated from the live bindings.
//!
//! Read from the keymap in force rather than from a written list, because a
//! rebound key described by its default would be worse than no sheet at all.

use gpui::prelude::*;
use gpui::{Context, MouseButton, SharedString, Window, div, px};

use super::kit;
use petunia_config::Theme;
use petunia_config::keys::Action;
use crate::theme::ActivePalette;

pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for Help {}

pub struct Help {
    bindings: Vec<(String, Action)>,
    focus: gpui::FocusHandle,
}

impl Help {
    pub fn new(bindings: Vec<(String, Action)>, cx: &mut Context<Self>) -> Self {
        Self {
            bindings,
            focus: cx.focus_handle(),
        }
    }
}

impl gpui::Focusable for Help {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Help {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();

        div()
            .id("help")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Hsla {
                a: 0.72,
                ..palette.background
            })
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .id("sheet")
                    .w(px(460.0))
                    .max_h(px(560.0))
                    .overflow_y_scroll()
                    .p_5()
                    .rounded(px(kit::RADIUS_LG))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    // Swallowed, so clicking the sheet itself does not dismiss it.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .pb_3()
                            .text_size(px(palette.typography.ui_size + 2.0))
                            .text_color(palette.text)
                            .child("Keyboard"),
                    )
                    .children(self.bindings.iter().map(|(chord, action)| {
                        row(chord, describe(*action), &palette)
                    })),
            )
    }
}

fn row(chord: &str, what: &'static str, palette: &Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .py_1p5()
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text_dim)
                .child(what),
        )
        .child(
            div()
                .flex_none()
                .px_2()
                .py_0p5()
                .rounded(px(5.0))
                .bg(palette.sunken)
                .font_family(palette.typography.mono.clone())
                .text_size(px(palette.typography.ui_size - 2.0))
                .text_color(palette.text)
                .child(SharedString::from(chord.to_owned())),
        )
}

/// What each action does, in the words a person would use for it.
fn describe(action: Action) -> &'static str {
    match action {
        Action::QuickSwitcher => "Jump to a conversation",
        Action::NewChat => "Start a conversation, or a group",
        Action::Search => "Search every conversation",
        Action::SearchThread => "Search this conversation",
        Action::FocusComposer => "Write a message",
        Action::ToggleSidebar => "Show or hide the conversation list",
        Action::ToggleDetails => "Show or hide the details panel",
        Action::ScrollUp => "Scroll up",
        Action::ScrollDown => "Scroll down",
        Action::ScrollToTop => "Go to the oldest loaded message",
        Action::ScrollToBottom => "Go to the newest message",
        Action::NextUnread => "Next unread conversation",
        Action::NextConversation => "Next conversation",
        Action::PreviousConversation => "Previous conversation",
        Action::MarkRead => "Mark this conversation read",
        Action::ReplyToLast => "Reply to the last message",
        Action::EditLast => "Edit your last message",
        Action::AttachFile => "Attach a file",
        Action::Cancel => "Close, or drop what the composer is carrying",
        Action::Help => "This sheet",
        Action::Settings => "Settings",
        Action::ThemePicker => "Change the theme",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use petunia_config::Keys;

    /// The sheet is generated from the live bindings, so every action the
    /// keymap can hold has to have something to say for itself.
    #[test]
    fn every_bound_action_is_described() {
        for (_, action) in Keys::default().listing() {
            assert!(!describe(action).is_empty(), "{action:?} has no description");
        }
    }

    #[test]
    fn the_listing_is_stable() {
        let keys = Keys::default();

        assert_eq!(keys.listing(), keys.listing());
        assert!(!keys.listing().is_empty());
    }
}
