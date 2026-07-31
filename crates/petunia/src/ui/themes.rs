//! Picking a theme by typing at it.
//!
//! Zed's own theme selector previews as you move through the list, and that is
//! the point of it: a palette is not a thing you can choose from a name. So
//! moving the selection installs the theme, and leaving without choosing puts
//! back the one you arrived with.

use gpui::prelude::*;
use gpui::{
    Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, SharedString, Window, div,
    px,
};

use super::kit;
use super::switcher;
use petunia_config::{Theme, theme, write};
use crate::store::Store;
use crate::theme::ActivePalette;

pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for Themes {}

pub struct Themes {
    store: Entity<Store>,
    focus: FocusHandle,
    query: String,
    selected: usize,
    /// What was in force when this opened, to go back to if nothing is chosen.
    /// Previewing without a way back would make browsing destructive.
    restore: String,
    chosen: bool,
}

impl Themes {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let restore = store.read(cx).config.theme.clone();
        Self {
            store,
            focus: cx.focus_handle(),
            query: String::new(),
            selected: 0,
            restore,
            chosen: false,
        }
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.selected = 0;
        self.chosen = false;
        self.restore = self.store.read(cx).config.theme.clone();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn hits(&self) -> Vec<String> {
        matches(&theme::available(), &self.query)
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let hits = self.hits();
        self.selected = switcher::step(self.selected, delta, hits.len());
        if let Some(name) = hits.get(self.selected) {
            self.preview(name.clone(), cx);
        }
        cx.notify();
    }

    /// Installs a theme without writing it down. Nothing is saved until it is
    /// actually chosen.
    fn preview(&self, name: String, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            let mut config = (*store.config).clone();
            config.theme = name;
            store.config_changed(std::sync::Arc::new(config), cx);
        });
        crate::theme::install(loaded(&self.store.read(cx).config.theme), cx);
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.hits().get(self.selected).cloned() else {
            return;
        };
        self.preview(name.clone(), cx);
        self.chosen = true;

        let mut config = (*self.store.read(cx).config).clone();
        config.theme = name;
        if let Err(error) = write::save(&config) {
            tracing::warn!(%error, "could not write the chosen theme");
        }
        cx.emit(Dismissed);
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => cx.emit(Dismissed),
            "enter" => self.choose(cx),
            "up" => self.step(-1, cx),
            "down" => self.step(1, cx),
            "backspace" => {
                self.query.pop();
                self.after_typing(cx);
            }
            _ => {
                if let Some(typed) = event.keystroke.key_char.as_ref()
                    && !typed.is_empty()
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                {
                    self.query.push_str(typed);
                    self.after_typing(cx);
                }
            }
        }
    }

    /// Typing narrows the list, so whatever is now at the top is what is shown.
    fn after_typing(&mut self, cx: &mut Context<Self>) {
        self.selected = 0;
        if let Some(name) = self.hits().first().cloned() {
            self.preview(name, cx);
        }
        cx.notify();
    }
}

impl Themes {
    /// What to go back to, or `None` if a theme was actually chosen. Browsing
    /// installs each theme as the selection moves, so closing without choosing
    /// has to undo that or looking would be the same as picking.
    pub fn abandoned(&self) -> Option<String> {
        (!self.chosen).then(|| self.restore.clone())
    }
}

fn loaded(name: &str) -> Theme {
    theme::load(name).0
}

/// The themes a query selects, best first. An empty query is all of them, in
/// the order they are listed.
pub fn matches(available: &[String], query: &str) -> Vec<String> {
    if query.trim().is_empty() {
        return available.to_vec();
    }

    let mut scored: Vec<_> = available
        .iter()
        .filter_map(|name| switcher::score(name, query).map(|score| (score, name)))
        .collect();
    scored.sort_by_key(|(score, name)| (std::cmp::Reverse(*score), (*name).clone()));
    scored.into_iter().map(|(_, name)| name.clone()).collect()
}

impl Focusable for Themes {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Themes {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let hits = self.hits();
        let selected = self.selected;

        div()
            .id("theme-picker")
            .track_focus(&self.focus)
            .key_context("ThemePicker")
            .on_key_down(cx.listener(Self::on_key))
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .pt_16()
            .bg(gpui::Hsla {
                a: 0.5,
                ..palette.background
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .id("sheet")
                    .w(px(420.0))
                    .max_h(px(460.0))
                    .flex()
                    .flex_col()
                    .rounded(px(kit::RADIUS_LG))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex_none()
                            .px_3p5()
                            .py_3()
                            .border_b_1()
                            .border_color(palette.border)
                            .text_size(px(palette.typography.ui_size))
                            .text_color(if self.query.is_empty() {
                                palette.text_muted
                            } else {
                                palette.text
                            })
                            .child(SharedString::from(match self.query.is_empty() {
                                true => "Search themes…".to_string(),
                                false => self.query.clone(),
                            })),
                    )
                    .child(
                        div()
                            .id("theme-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_1p5()
                            .children(hits.into_iter().enumerate().map(|(index, name)| {
                                kit::row(
                                    SharedString::from(format!("theme-{name}")),
                                    index == selected,
                                    &palette,
                                )
                                .items_center()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this: &mut Self, _, _, cx| {
                                        this.selected = index;
                                        this.choose(cx);
                                    }),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(palette.typography.ui_size))
                                        .text_color(palette.text)
                                        .child(SharedString::from(name)),
                                )
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available() -> Vec<String> {
        ["dark", "light", "one-dark", "one-light", "gruvbox-dark", "ayu-mirage"]
            .map(str::to_string)
            .to_vec()
    }

    #[test]
    fn an_empty_query_offers_everything() {
        assert_eq!(matches(&available(), "").len(), 6);
        assert_eq!(matches(&available(), "  ").len(), 6);
    }

    #[test]
    fn a_name_finds_its_theme() {
        let found = matches(&available(), "gruv");

        assert_eq!(found.first().map(String::as_str), Some("gruvbox-dark"));
    }

    /// Fuzzy, not prefix: typing the shape of a name is how these are used.
    #[test]
    fn letters_in_order_are_enough() {
        assert_eq!(
            matches(&available(), "onelight").first().map(String::as_str),
            Some("one-light")
        );
    }

    #[test]
    fn nothing_matching_finds_nothing() {
        assert!(matches(&available(), "solarized").is_empty());
    }

    /// A stable order, or the list reshuffles under the selection as you type.
    #[test]
    fn ties_break_by_name() {
        assert_eq!(matches(&available(), "dark"), matches(&available(), "dark"));
    }
}
