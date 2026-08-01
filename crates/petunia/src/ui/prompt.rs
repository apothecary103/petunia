//! Asking for one line of text.
//!
//! Every single line goes through this one — a folder's name, a nickname, a
//! group's title — and it is here rather than inline in the sidebar because a
//! menu cannot hold a text field: the menu closes on the click that would focus
//! it.

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, SharedString, Subscription, Window, div, px};
use gpui_component::input::{Input, InputEvent, InputState};

use super::kit;
use crate::theme::ActivePalette;

pub struct Dismissed;

/// What was typed. Never empty and never only spaces: an unnamed folder is not
/// a folder, so the prompt refuses rather than making one.
#[derive(Debug, Clone)]
pub struct Answered(pub String);

impl gpui::EventEmitter<Dismissed> for Prompt {}
impl gpui::EventEmitter<Answered> for Prompt {}

pub struct Prompt {
    title: SharedString,
    confirm: SharedString,
    /// One line under the field, for the thing somebody would otherwise have to
    /// be told elsewhere or not at all. Absent unless it is asked for, since a
    /// prompt with nothing to explain must not reserve a line for it.
    note: Option<SharedString>,
    input: Entity<InputState>,
    focus: gpui::FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Prompt {
    pub fn new(
        title: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        confirm: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.answer(cx);
                }
            },
        )];

        Self {
            title: title.into(),
            confirm: confirm.into(),
            note: None,
            input,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn note(mut self, note: impl Into<SharedString>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn answer(&mut self, cx: &mut Context<Self>) {
        let typed = self.input.read(cx).value().trim().to_string();
        if typed.is_empty() {
            return;
        }
        cx.emit(Answered(typed));
        cx.emit(Dismissed);
    }
}

impl gpui::Focusable for Prompt {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Prompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let ready = !self.input.read(cx).value().trim().is_empty();

        kit::scrim(&palette)
            .id("prompt")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                kit::dialog(360.0, &palette)
                    .child(
                        div()
                            .text_size(px(palette.typography.ui_size + 1.0))
                            .text_color(palette.text)
                            .child(self.title.clone()),
                    )
                    .child(
                        kit::field(&palette)
                            .child(Input::new(&self.input).appearance(false).bordered(false)),
                    )
                    .children(self.note.clone().map(|note| {
                        div()
                            .text_size(px(palette.typography.ui_size - 1.0))
                            .text_color(palette.text_muted)
                            .child(note)
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
                                "confirm",
                                self.confirm.clone(),
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
