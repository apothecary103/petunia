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
    /// So the arrow keys can bring the selection back into view.
    scroll: gpui::ScrollHandle,
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
            scroll: gpui::ScrollHandle::new(),
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
        self.scroll.scroll_to_item(self.selected);
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
                        query: &self.answered,
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
            .occlude()
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
                                    .child(Input::new(&self.query).appearance(false).bordered(false)),
                            )
                            // What was found, beside what was asked. A result list
                            // that is exactly as long as the limit needs to say so.
                            .when(searching && !self.hits.is_empty(), |this| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(palette.typography.ui_size - 2.0))
                                        .text_color(palette.text_muted)
                                        .child(SharedString::from(counted(self.hits.len()))),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("results")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
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

/// How many were found, and whether that is all of them. The query returns the
/// newest `LIMIT`, so a full page means "at least this many" rather than "this
/// many" -- and saying the wrong one of those is how a search convinces somebody
/// a message is not there.
fn counted(hits: usize) -> String {
    match hits >= petunia_signal::db::search::LIMIT as usize {
        true => format!("{hits}+ matches"),
        false if hits == 1 => "1 match".to_owned(),
        false => format!("{hits} matches"),
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
    /// What was searched for, so the row can show where it matched.
    query: &'a str,
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
                    .child(matched(
                        &result.hit.body.replace('\n', " "),
                        result.query,
                        palette,
                    )),
            ),
    )
}

/// The line a hit matched on, with the words that matched picked out. A list of
/// twenty results all reading "…and then I said…" is a list you have to read;
/// marking the match is what makes it a list you can scan.
pub fn matched(body: &str, query: &str, palette: &Theme) -> gpui::StyledText {
    let highlight = gpui::HighlightStyle {
        color: Some(palette.accent),
        font_weight: Some(kit::EMPHASIS),
        ..Default::default()
    };

    gpui::StyledText::new(body.to_owned())
        .with_highlights(occurrences(body, query).into_iter().map(|range| (range, highlight)))
}

/// Every place `query` occurs in `body`, ignoring case. The same match the
/// database made -- a `LIKE '%query%'` -- so a row cannot claim a match the
/// search did not make.
fn occurrences(body: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let (haystack, needle) = (body.to_lowercase(), query.to_lowercase());
    // Lowercasing can change a string's length, so a position in the lowered
    // copy is not a position in the original. Only ranges that still land on a
    // character boundary of the original are used, which leaves the rare
    // multi-byte-folding case unmarked rather than panicking in the renderer.
    if haystack.len() != body.len() {
        return Vec::new();
    }

    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = haystack[at..].find(&needle) {
        let start = at + offset;
        let end = start + needle.len();
        if !body.is_char_boundary(start) || !body.is_char_boundary(end) {
            break;
        }
        found.push(start..end);
        at = end;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_occurrence_is_marked_and_matching_ignores_case() {
        assert_eq!(
            occurrences("Deploy, then deploy again", "DEPLOY"),
            [0..6, 13..19]
        );
    }

    #[test]
    fn an_empty_query_marks_nothing() {
        assert!(occurrences("anything", "   ").is_empty());
        assert!(occurrences("anything", "").is_empty());
    }

    /// Every range is handed to the renderer, which panics rather than truncates
    /// if one lands inside a character.
    #[test]
    fn a_range_never_lands_inside_a_character() {
        for range in occurrences("héllo wörld, héllo", "héllo") {
            assert!("héllo wörld, héllo".is_char_boundary(range.start), "{range:?}");
            assert!("héllo wörld, héllo".is_char_boundary(range.end), "{range:?}");
        }
    }

    /// A full page is "at least this many": the query returns the newest few, and
    /// reporting that as the total is how a search talks somebody out of looking.
    #[test]
    fn a_full_page_of_results_says_there_may_be_more() {
        let limit = petunia_signal::db::search::LIMIT as usize;

        assert_eq!(counted(1), "1 match");
        assert_eq!(counted(3), "3 matches");
        assert_eq!(counted(limit), format!("{limit}+ matches"));
    }
}
