//! Asking for one line of text.
//!
//! There is exactly one of these — naming a folder — and it is here rather than
//! inline in the sidebar because a menu cannot hold a text field: the menu
//! closes on the click that would focus it.

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
            input,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
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

        div()
            .id("prompt")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Hsla {
                a: 0.6,
                ..palette.background
            })
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .w(px(360.0))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .rounded(px(kit::RADIUS_LG))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(palette.typography.ui_size + 1.0))
                            .text_color(palette.text)
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .px_2p5()
                            .py_2()
                            .rounded(px(kit::RADIUS))
                            .bg(palette.sunken)
                            .border_1()
                            .border_color(palette.border)
                            .child(Input::new(&self.input).appearance(false).bordered(false)),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button(
                                "cancel",
                                "Cancel",
                                false,
                                &palette,
                                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
                            ))
                            .child(button(
                                "confirm",
                                self.confirm.clone(),
                                ready,
                                &palette,
                                cx.listener(|this: &mut Self, _, _, cx| this.answer(cx)),
                            )),
                    ),
            )
    }
}

fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    primary: bool,
    palette: &crate::config::Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded(px(kit::RADIUS))
        .cursor_pointer()
        .when(primary, |this| this.bg(palette.accent))
        .when(!primary, |this| {
            this.border_1()
                .border_color(palette.border)
                .hover(|this| this.bg(palette.hover))
        })
        .text_size(px(palette.typography.ui_size))
        .text_color(if primary {
            palette.on_accent
        } else {
            palette.text_dim
        })
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label.into())
}
