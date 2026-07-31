//! Finding a message, in one conversation or in all of them.
//!
//! One surface for both, because they differ only in what they are asked and
//! what a result says: a global search names the conversation a hit came from,
//! a scoped one does not, since you already know.

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, SharedString, Subscription, Window, div, px};
use gpui_component::input::{Input, InputEvent, InputState};

use super::avatar::avatar;
use super::kit;
use super::relative;
use petunia_config::Theme;
use petunia_data::Thread;
use petunia_signal::Command;
use petunia_signal::db::search::Hit;
use crate::store::Store;
use crate::theme::ActivePalette;

pub struct Dismissed;

/// A result was chosen. The workspace opens the conversation, because the search
/// does not own what is on screen.
#[derive(Debug, Clone)]
pub struct Chosen(pub Hit);

impl gpui::EventEmitter<Dismissed> for Search {}
impl gpui::EventEmitter<Chosen> for Search {}

/// How wide a net to cast. `Thread` rather than a flag, so a scoped search
/// cannot drift out of step with the conversation it was opened over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Everywhere,
    Thread(Thread),
}

pub struct Search {
    store: Entity<Store>,
    query: Entity<InputState>,
    scope: Scope,
    hits: Vec<Hit>,
    /// What the results on screen answer. A slower search finishing after a
    /// faster one would otherwise replace newer results with older ones.
    answered: String,
    selected: usize,
    focus: gpui::FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Search {
    pub fn new(
        store: Entity<Store>,
        scope: Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match scope {
                Scope::Everywhere => "Search every conversation",
                Scope::Thread(_) => "Search this conversation",
            })
        });

        let subscriptions = vec![
            cx.subscribe_in(&query, window, Self::on_query),
            cx.subscribe(&store, |this, _, event: &crate::store::StoreEvent, cx| {
                if let crate::store::StoreEvent::Found { query, hits } = event {
                    this.receive(query, hits, cx);
                }
            }),
        ];

        Self {
            store,
            query,
            scope,
            hits: Vec::new(),
            answered: String::new(),
            selected: 0,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Raising it again refocuses and clears, which is what pressing the
    /// shortcut twice is asking for.
    pub fn reset(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = scope;
        self.hits.clear();
        self.answered.clear();
        self.selected = 0;
        self.query.update(cx, |query, cx| {
            query.set_value("", window, cx);
            query.focus(window, cx);
        });
        cx.notify();
    }

    fn on_query(
        &mut self,
        _query: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.ask(cx),
            InputEvent::PressEnter { .. } => self.choose(cx),
            _ => {}
        }
    }

    fn ask(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_string();
        let within = match &self.scope {
            Scope::Everywhere => None,
            Scope::Thread(thread) => Some(thread.clone()),
        };

        if query.trim().is_empty() {
            self.hits.clear();
            self.answered.clear();
            cx.notify();
            return;
        }
        self.store
            .update(cx, |store, _| store.send(Command::Search { query, within }));
    }

    fn receive(&mut self, query: &str, hits: &[Hit], cx: &mut Context<Self>) {
        // Only if it answers what is typed now: a slow search finishing late
        // would otherwise overwrite newer results with older ones.
        if self.query.read(cx).value().trim() != query.trim() {
            return;
        }
        self.answered = query.to_owned();
        self.hits = hits.to_vec();
        self.selected = 0;
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.hits.is_empty() {
            return;
        }
        let count = self.hits.len() as isize;
        self.selected = (((self.selected as isize + by) % count + count) % count) as usize;
        cx.notify();
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        if let Some(hit) = self.hits.get(self.selected).cloned() {
            cx.emit(Chosen(hit));
            cx.emit(Dismissed);
        }
    }
}

impl gpui::Focusable for Search {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Search {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let store = self.store.read(cx);
        let state = store.state();
        let searching = !self.query.read(cx).value().trim().is_empty();
        let scoped = matches!(self.scope, Scope::Thread(_));

        let rows: Vec<_> = self
            .hits
            .iter()
            .enumerate()
            .map(|(index, hit)| {
                let name = state
                    .map(|state| state.title(&hit.thread))
                    .unwrap_or_default();
                let who = state
                    .map(|state| state.sender_name(hit.sender))
                    .unwrap_or_default();
                let picture = state.and_then(|state| state.avatar(&hit.thread));

                row(
                    Result_ {
                        hit,
                        thread_name: &name,
                        sender: &who,
                        picture,
                        scoped,
                        selected: index == self.selected,
                        palette: &palette,
                    },
                    cx.listener(move |this, _, _, cx| {
                        this.selected = index;
                        this.choose(cx);
                    }),
                )
            })
            .collect();

        div()
            .id("search")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .pt_16()
            .bg(gpui::Hsla {
                a: 0.55,
                ..palette.background
            })
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_action(cx.listener(|this, _: &crate::actions::ScrollDown, _, cx| {
                this.step(1, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::ScrollUp, _, cx| {
                this.step(-1, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .id("sheet")
                    .w(px(560.0))
                    .max_h(px(520.0))
                    .flex()
                    .flex_col()
                    .rounded(px(kit::RADIUS_LG))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .overflow_hidden()
                    // Swallowed, so clicking the sheet does not dismiss it.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex_none()
                            .px_3p5()
                            .py_3()
                            .border_b_1()
                            .border_color(palette.border)
                            .child(Input::new(&self.query).appearance(false).bordered(false)),
                    )
                    .child(
                        div()
                            .id("results")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_1p5()
                            .children(rows),
                    )
                    .when(searching && self.hits.is_empty(), |this| {
                        this.child(note("Nothing found.", &palette))
                    })
                    .when(!searching, |this| {
                        this.child(note(
                            match scoped {
                                true => "Type to search this conversation.",
                                false => "Type to search every conversation.",
                            },
                            &palette,
                        ))
                    }),
            )
    }
}

fn note(text: &'static str, palette: &Theme) -> gpui::Div {
    div()
        .px_4()
        .py_5()
        .text_size(px(palette.typography.ui_size - 1.0))
        .text_color(palette.text_muted)
        .child(text)
}

/// Everything one result row draws.
struct Result_<'a> {
    hit: &'a Hit,
    thread_name: &'a str,
    sender: &'a str,
    picture: Option<&'a std::path::Path>,
    /// A scoped search already knows which conversation this is.
    scoped: bool,
    selected: bool,
    palette: &'a Theme,
}

fn row(
    result: Result_<'_>,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let palette = result.palette;
    let heading = match result.scoped {
        true => result.sender.to_owned(),
        false => format!("{} · {}", result.thread_name, result.sender),
    };

    kit::row(
        SharedString::from(format!("hit-{}", result.hit.timestamp)),
        result.selected,
        palette,
    )
    .on_mouse_down(MouseButton::Left, on_click)
    .child(
        div()
            .pt_0p5()
            .child(avatar(
                result.picture,
                result.thread_name,
                result.hit.thread.seed(),
                26.0,
                palette,
            )),
    )
    .child(
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(palette.typography.ui_size - 1.0))
                            .text_color(palette.text_dim)
                            .child(SharedString::from(heading)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(palette.typography.ui_size - 3.0))
                            .text_color(palette.text_muted)
                            .child(SharedString::from(relative::short(result.hit.timestamp))),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text)
                    .child(SharedString::from(result.hit.body.replace('\n', " "))),
            ),
    )
}
