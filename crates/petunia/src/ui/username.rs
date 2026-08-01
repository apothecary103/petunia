//! Choosing a username: a name, a dot, and a number.
//!
//! One field asked for both halves and left the dot to be typed, which made a
//! two-part identity look like one string with a punctuation rule nobody had been
//! told. Two fields with the separator drawn between them say the shape without a
//! sentence explaining it — and the second one may be left blank, which is how you
//! ask Signal to pick a free number rather than naming one.
//!
//! Neither half is a free-text field pretending to accept anything: the number
//! takes digits and the name takes what Signal allows a name, filtered on the way
//! in, because finding that out from a round trip to the server is finding it out
//! too late — and because a field showing `Wren` while `wren` is what would be
//! asked for is a field lying about its own contents.

use gpui::prelude::*;
use gpui::{App, Context, Entity, MouseButton, Subscription, Window, div, px};
use gpui_component::input::{Input, InputEvent, InputState};

use super::kit;
use crate::theme::ActivePalette;

pub struct Dismissed;

/// The username as Signal wants it: `name` alone to have a number picked, or
/// `name.number` to ask for that exact one.
#[derive(Debug, Clone)]
pub struct Answered(pub String);

impl gpui::EventEmitter<Dismissed> for Username {}
impl gpui::EventEmitter<Answered> for Username {}

/// Signal's own bounds on the nickname half.
const SHORTEST: usize = 3;
const LONGEST: usize = 32;

/// And on the number: Signal issues two to nine digits.
const DIGITS: usize = 9;

pub struct Username {
    name: Entity<InputState>,
    number: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    focus: gpui::FocusHandle,
}

impl Username {
    pub fn new(current: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Prefilled with what this account already has, since "change" almost
        // always means changing one half.
        let (was_name, was_number) = split(current.unwrap_or_default());

        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("name")
                .default_value(was_name)
        });
        let number = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("00")
                .default_value(was_number)
        });

        let subscriptions = vec![
            cx.subscribe_in(
                &name,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.answer(cx),
                    InputEvent::Change => tidy(&this.name.clone(), nickname, window, cx),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &number,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.answer(cx),
                    InputEvent::Change => tidy(&this.number.clone(), digits, window, cx),
                    _ => {}
                },
            ),
        ];

        Self {
            name,
            number,
            _subscriptions: subscriptions,
            focus: cx.focus_handle(),
        }
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.name.update(cx, |input, cx| input.focus(window, cx));
    }

    fn wanted(&self, cx: &App) -> Option<String> {
        let name = self.name.read(cx).value().to_string();
        if name.chars().count() < SHORTEST {
            return None;
        }
        let number = self.number.read(cx).value().to_string();

        Some(match number.is_empty() {
            true => name,
            false => format!("{name}.{number}"),
        })
    }

    /// The one thing worth saying under the fields, or nothing.
    fn note(&self, cx: &App) -> Option<&'static str> {
        match self.wanted(cx) {
            None => Some("A name is at least three characters."),
            Some(_) if self.number.read(cx).value().is_empty() => {
                Some("Leave the number blank to have one picked.")
            }
            Some(_) => None,
        }
    }

    fn answer(&mut self, cx: &mut Context<Self>) {
        let Some(wanted) = self.wanted(cx) else {
            return;
        };
        cx.emit(Answered(wanted));
        cx.emit(Dismissed);
    }
}

/// Rewrites a field to the characters it accepts, and only when there is
/// something to take out -- setting the value on every keystroke would move the
/// caret to the end of every edit made in the middle.
fn tidy(
    field: &Entity<InputState>,
    accepted: fn(&str) -> String,
    window: &mut Window,
    cx: &mut App,
) {
    let typed = field.read(cx).value().to_string();
    let cleaned = accepted(&typed);
    if cleaned != typed {
        field.update(cx, |input, cx| input.set_value(cleaned, window, cx));
    }
}

/// A nickname as Signal will take one: lowercase letters, digits and
/// underscores.
fn nickname(typed: &str) -> String {
    typed
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(LONGEST)
        .collect()
}

/// A discriminator, which is a number and nothing else.
fn digits(typed: &str) -> String {
    typed.chars().filter(char::is_ascii_digit).take(DIGITS).collect()
}

/// Splits what an account already has into the two fields. A stored username is
/// always both halves, but it is read off a server record rather than built here,
/// so a missing dot is a case rather than an impossibility.
fn split(username: &str) -> (String, String) {
    match username.rsplit_once('.') {
        Some((name, number)) if number.chars().all(|c| c.is_ascii_digit()) => {
            (name.to_owned(), number.to_owned())
        }
        _ => (username.to_owned(), String::new()),
    }
}

impl gpui::Focusable for Username {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Username {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let ready = self.wanted(cx).is_some();

        kit::scrim(&palette)
            .id("username")
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
                            .child("Choose a username"),
                    )
                    // One box, not two beside each other: the two halves are one
                    // identity, and the separator belongs inside the thing it
                    // separates rather than floating between two controls that
                    // then read as unrelated.
                    .child(
                        kit::field(&palette)
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.name).appearance(false).bordered(false)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(palette.text_muted)
                                    .child("."),
                            )
                            .child(
                                div().flex_none().w(px(64.0)).child(
                                    Input::new(&self.number).appearance(false).bordered(false),
                                ),
                            ),
                    )
                    // Why the button is quiet, or what the blank number will do.
                    // The line keeps its height either way: one that came and
                    // went moved the buttons out from under the pointer, and a
                    // "Set" that goes dead without saying why is a control with
                    // no reason given.
                    .child(
                        div()
                            .h(px(palette.typography.ui_size))
                            .text_size(px(palette.typography.ui_size - 2.0))
                            .text_color(palette.text_muted)
                            .children(self.note(cx)),
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
                                "Set",
                                match ready {
                                    true => kit::Intent::Primary,
                                    false => kit::Intent::Quiet,
                                },
                                &palette,
                                cx.listener(|this: &mut Self, _, _, cx| this.answer(cx)),
                            )),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field shows what would be asked for, so what it accepts is what
    /// Signal accepts and the folding happens on the way in.
    #[test]
    fn a_name_is_folded_and_filtered_as_it_is_typed() {
        assert_eq!(nickname("Wren"), "wren");
        assert_eq!(nickname("wren tanaka!"), "wrentanaka");
        assert_eq!(nickname("wren_01"), "wren_01");
        assert_eq!(nickname(&"w".repeat(LONGEST + 10)), "w".repeat(LONGEST));
    }

    #[test]
    fn a_discriminator_is_digits_and_nothing_else() {
        assert_eq!(digits("48a23"), "4823");
        assert_eq!(digits("1234567890123"), "123456789");
    }

    #[test]
    fn a_stored_username_splits_into_its_halves() {
        assert_eq!(split("wren.01"), ("wren".into(), "01".into()));
        assert_eq!(split("wren.4823"), ("wren".into(), "4823".into()));
    }

    /// A nickname may contain a dot of its own; the *last* one is the separator,
    /// and only when what follows it is a number.
    #[test]
    fn only_a_trailing_number_is_a_discriminator() {
        assert_eq!(split("a.b.01"), ("a.b".into(), "01".into()));
        assert_eq!(split("wren.tanaka"), ("wren.tanaka".into(), String::new()));
        assert_eq!(split("wren"), ("wren".into(), String::new()));
        assert_eq!(split(""), (String::new(), String::new()));
    }
}
