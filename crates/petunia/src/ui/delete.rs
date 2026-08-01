//! Which deletion, asked the way Signal asks it.
//!
//! There are two, and they are not degrees of the same thing. "Delete for me"
//! takes the message off this account and tells nobody; "delete for everyone"
//! asks every recipient to withdraw it and leaves a tombstone in its place that
//! says one was withdrawn. Offering them as one "Delete" and picking for the
//! reader would be picking the wrong one about half the time.
//!
//! So the menu item opens this instead of doing anything, and the buttons are the
//! two verbs. The second is only drawn when it can work: Signal honours a remote
//! delete from the message's own author, within a day of it being sent, and
//! ignores it otherwise — a button that quietly did nothing would be worse than
//! no button, and the line under the title says why it is not there.

use gpui::prelude::*;
use gpui::{Context, MouseButton, Window, div, px};

use super::kit;
use crate::theme::ActivePalette;
use petunia_data::MessageId;

pub struct Dismissed;

/// Which one was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chose {
    ForMe,
    ForEveryone,
}

pub struct Chosen(pub MessageId, pub Chose);

impl gpui::EventEmitter<Dismissed> for Delete {}
impl gpui::EventEmitter<Chosen> for Delete {}

/// How long after sending Signal will still carry out a remote delete. Its own
/// window, and the servers enforce it: asking outside it is a message every
/// recipient ignores.
const WITHDRAWABLE: u64 = 24 * 60 * 60 * 1000;

pub struct Delete {
    target: MessageId,
    /// Whether a remote delete would be honoured, which is our own message
    /// inside the window.
    withdrawable: bool,
    own: bool,
    focus: gpui::FocusHandle,
}

impl Delete {
    pub fn new(target: MessageId, own: bool, now: u64, cx: &mut Context<Self>) -> Self {
        Self {
            target,
            withdrawable: own && now.saturating_sub(target.timestamp) < WITHDRAWABLE,
            own,
            focus: cx.focus_handle(),
        }
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    /// Why "delete for everyone" is not on offer, when it is not. Nothing at all
    /// when it is: a dialog does not need to explain the button it is showing.
    fn caveat(&self) -> Option<&'static str> {
        match (self.own, self.withdrawable) {
            (_, true) => None,
            (true, false) => Some("This is too old to withdraw from everybody else."),
            (false, false) => Some("Only the sender can withdraw a message from everybody."),
        }
    }
}

impl gpui::Focusable for Delete {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Delete {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let target = self.target;

        kit::scrim(&palette)
            .id("delete")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                kit::dialog(400.0, &palette)
                    .child(
                        div()
                            .text_size(px(palette.typography.ui_size + 1.0))
                            .text_color(palette.text)
                            .child("Delete message"),
                    )
                    .children(self.caveat().map(|caveat| {
                        div()
                            .text_size(px(palette.typography.ui_size - 1.0))
                            .line_height(px((palette.typography.ui_size - 1.0) * 1.5))
                            .text_color(palette.text_muted)
                            .child(caveat)
                    }))
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
                                "for-me",
                                "Delete for me",
                                kit::Intent::Quiet,
                                &palette,
                                cx.listener(move |_: &mut Self, _, _, cx| {
                                    cx.emit(Chosen(target, Chose::ForMe));
                                    cx.emit(Dismissed);
                                }),
                            ))
                            .children(self.withdrawable.then(|| {
                                kit::button(
                                    "for-everyone",
                                    "Delete for everyone",
                                    kit::Intent::Danger,
                                    &palette,
                                    cx.listener(move |_: &mut Self, _, _, cx| {
                                        cx.emit(Chosen(target, Chose::ForEveryone));
                                        cx.emit(Dismissed);
                                    }),
                                )
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn target(sent: u64) -> MessageId {
        MessageId {
            timestamp: sent,
            sender: Uuid::nil(),
        }
    }

    /// The arithmetic worth pinning, since it decides whether a button that
    /// cannot work is drawn.
    #[test]
    fn only_your_own_message_inside_the_day_can_be_withdrawn() {
        let withdrawable = |own, sent, now: u64| {
            own && now.saturating_sub(target(sent).timestamp) < WITHDRAWABLE
        };

        assert!(withdrawable(true, 1_000, 1_000));
        assert!(withdrawable(true, 1_000, 1_000 + WITHDRAWABLE - 1));
        assert!(!withdrawable(true, 1_000, 1_000 + WITHDRAWABLE));
        assert!(!withdrawable(false, 1_000, 1_000));
    }

    /// A clock that disagrees with the sender's must not make the arithmetic
    /// panic: a message stamped in the future is a message somebody else's
    /// device dated.
    #[test]
    fn a_message_from_the_future_is_not_a_subtraction_overflow() {
        let now = 1_000u64;
        assert_eq!(now.saturating_sub(target(9_999).timestamp), 0);
    }
}
