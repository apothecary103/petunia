//! Building a poll before it is sent.
//!
//! A fixed run of option fields rather than a `Vec` that grows one at a time:
//! `InputState` is an entity, and inserting one mid-list would mean every
//! field after it re-subscribing under a new key. Ten is Signal's own limit.

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, Subscription, Window, div, px};
use gpui_component::input::{Input, InputEvent, InputState};

use super::kit;
use crate::store::Store;
use crate::theme::ActivePalette;
use petunia_data::Thread;

const OPTIONS: usize = 10;
/// Started with room for a real choice without the dialog opening tall enough
/// to cover the conversation behind it.
const VISIBLE: usize = 2;

pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for PollComposer {}

pub struct PollComposer {
    store: Entity<Store>,
    thread: Thread,
    question: Entity<InputState>,
    options: Vec<Entity<InputState>>,
    visible: usize,
    allow_multiple: bool,
    focus: gpui::FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PollComposer {
    pub fn new(store: Entity<Store>, thread: Thread, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let question = cx.new(|cx| InputState::new(window, cx).placeholder("Ask a question"));
        let options: Vec<_> = (0..OPTIONS)
            .map(|at| {
                cx.new(|cx| InputState::new(window, cx).placeholder(format!("Option {}", at + 1)))
            })
            .collect();

        let subscriptions = vec![cx.subscribe_in(
            &question,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.create(window, cx);
                }
            },
        )];

        Self {
            store,
            thread,
            question,
            options,
            visible: VISIBLE,
            allow_multiple: false,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.question.update(cx, |input, cx| input.focus(window, cx));
    }

    fn add_option(&mut self, cx: &mut Context<Self>) {
        self.visible = (self.visible + 1).min(OPTIONS);
        cx.notify();
    }

    fn filled(&self, cx: &Context<Self>) -> Vec<String> {
        self.options[..self.visible]
            .iter()
            .map(|option| option.read(cx).value().trim().to_string())
            .filter(|value| !value.is_empty())
            .collect()
    }

    fn ready(&self, cx: &Context<Self>) -> bool {
        !self.question.read(cx).value().trim().is_empty() && self.filled(cx).len() >= 2
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        let question = self.question.read(cx).value().trim().to_string();
        let options = self.filled(cx);
        let allow_multiple = self.allow_multiple;
        let thread = self.thread.clone();

        self.store
            .update(cx, |store, cx| store.send_poll(thread, question, options, allow_multiple, cx));
        cx.emit(Dismissed);
        let _ = window;
    }
}

impl gpui::Focusable for PollComposer {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for PollComposer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let ready = self.ready(cx);
        let visible = self.visible;

        kit::scrim(&palette)
            .id("poll-composer")
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
                            .child("Create a poll"),
                    )
                    .child(field(&self.question, &palette))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .children(
                                self.options[..visible]
                                    .iter()
                                    .map(|option| field(option, &palette)),
                            )
                            .when(visible < OPTIONS, |this| {
                                this.child(
                                    div()
                                        .id("add-option")
                                        .cursor_pointer()
                                        .text_size(px(palette.typography.ui_size - 1.0))
                                        .text_color(palette.text_dim)
                                        .hover(|this| this.text_color(palette.text))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| this.add_option(cx)),
                                        )
                                        .child("Add an option"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("allow-multiple")
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.allow_multiple = !this.allow_multiple;
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .size(px(16.0))
                                    .rounded(px(4.0))
                                    .border_1()
                                    .border_color(match self.allow_multiple {
                                        true => palette.accent,
                                        false => palette.border,
                                    })
                                    .when(self.allow_multiple, |this| this.bg(palette.accent)),
                            )
                            .child(
                                div()
                                    .text_size(px(palette.typography.ui_size - 1.0))
                                    .text_color(palette.text_dim)
                                    .child("Allow multiple answers"),
                            ),
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
                                "create",
                                "Create",
                                match ready {
                                    true => kit::Intent::Primary,
                                    false => kit::Intent::Quiet,
                                },
                                &palette,
                                cx.listener(|this, _, window, cx| this.create(window, cx)),
                            )),
                    ),
            )
    }
}

fn field(input: &Entity<InputState>, palette: &petunia_config::Theme) -> impl IntoElement {
    kit::field(palette).child(Input::new(input).appearance(false).bordered(false))
}
