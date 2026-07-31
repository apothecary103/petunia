//! Editing `config.toml` in the app.
//!
//! The settings window covers what has a control; this covers everything,
//! including the keys that have none. It is the same file either way — settings
//! writes it, the watcher reloads it, and so does this.
//!
//! It refuses to save something the loader would reject. A config editor that
//! writes a broken file and leaves you to find out at the next start is worse
//! than no config editor.

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, SharedString, Subscription, Window, div, px};
use gpui_component::IconName;
use gpui_component::input::{Input, InputEvent, InputState};

use super::kit;
use petunia_config::{Config, config_path};
use crate::theme::ActivePalette;

pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for Editor {}

pub struct Editor {
    text: Entity<InputState>,
    /// What is wrong with what is typed, if anything. Checked as you type rather
    /// than on save, so a mistake is visible where it was made.
    problem: Option<String>,
    /// Whether what is on screen differs from what is on disk.
    dirty: bool,
    saved: bool,
    focus: gpui::FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Editor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Whatever is actually on disk, not what the loader made of it: this
        // edits the file, including the comments the loader throws away.
        let contents = std::fs::read_to_string(config_path()).unwrap_or_default();

        let text = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .multi_line(true)
                .code_editor("toml")
                .line_number(true);
            state.set_value(contents, window, cx);
            state
        });

        let subscriptions = vec![cx.subscribe_in(
            &text,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.check(cx);
                }
            },
        )];

        Self {
            text,
            problem: None,
            dirty: false,
            saved: false,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.text.update(cx, |text, cx| text.focus(window, cx));
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        let typed = self.text.read(cx).value().to_string();
        self.dirty = true;
        self.saved = false;
        self.problem = match toml::from_str::<Config>(&typed) {
            Ok(_) => None,
            Err(error) => Some(describe(&error)),
        };
        cx.notify();
    }

    /// Refuses to write something the loader would reject.
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.problem.is_some() {
            return;
        }
        let typed = self.text.read(cx).value().to_string();
        let path = config_path();

        let written = std::fs::create_dir_all(path.parent().unwrap_or(&path))
            .and_then(|()| std::fs::write(&path, typed));

        match written {
            Ok(()) => {
                self.dirty = false;
                self.saved = true;
            }
            Err(error) => self.problem = Some(format!("could not write the file: {error}")),
        }
        cx.notify();
    }
}

/// Points at the line, because "invalid type" with no location is not
/// actionable in a file you are looking at.
fn describe(error: &toml::de::Error) -> String {
    match error.span() {
        Some(span) => format!("line {}: {}", span.start, error.message().replace('\n', " ")),
        None => error.message().replace('\n', " "),
    }
}

impl gpui::Focusable for Editor {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let saveable = self.problem.is_none() && self.dirty;

        div()
            .id("editor")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Hsla {
                a: 0.7,
                ..palette.background
            })
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .w(px(720.0))
                    .h(px(620.0))
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
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(palette.border)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(palette.typography.ui_size))
                                    .text_color(palette.text_dim)
                                    .child(SharedString::from(
                                        config_path().display().to_string(),
                                    )),
                            )
                            .child(kit::icon_button(
                                "close-editor",
                                IconName::Close,
                                &palette,
                                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .p_2()
                            .font_family(palette.typography.mono.clone())
                            .text_size(px(palette.typography.ui_size))
                            .child(
                                Input::new(&self.text)
                                    .appearance(false)
                                    .bordered(false)
                                    .h_full(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .py_3()
                            .border_t_1()
                            .border_color(palette.border)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(palette.typography.ui_size - 1.0))
                                    .text_color(match (&self.problem, self.saved) {
                                        (Some(_), _) => palette.danger,
                                        (None, true) => palette.success,
                                        _ => palette.text_muted,
                                    })
                                    .child(SharedString::from(match (&self.problem, self.saved) {
                                        (Some(problem), _) => problem.clone(),
                                        (None, true) => "Saved. The change is already applied.".into(),
                                        (None, false) => {
                                            "Saving reloads the app, as editing the file by hand \
                                             does."
                                                .to_string()
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .id("save")
                                    .flex_none()
                                    .px_3()
                                    .py_1p5()
                                    .rounded(px(kit::RADIUS))
                                    .when(saveable, |this| {
                                        this.cursor_pointer().bg(palette.accent)
                                    })
                                    .when(!saveable, |this| {
                                        this.border_1().border_color(palette.border)
                                    })
                                    .text_size(px(palette.typography.ui_size))
                                    .text_color(match saveable {
                                        true => palette.on_accent,
                                        false => palette.text_muted,
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this: &mut Self, _, _, cx| this.save(cx)),
                                    )
                                    .child("Save"),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A broken file has to be refused with something that says where, or the
    /// editor is a way to write a config you find out about at the next start.
    #[test]
    fn a_parse_error_says_where_it_is() {
        let error = toml::from_str::<Config>("theme = \n").unwrap_err();

        let described = describe(&error);
        assert!(described.contains("line"), "{described}");
    }

    #[test]
    fn an_unknown_key_is_a_problem() {
        assert!(toml::from_str::<Config>("[sidebar]\nwdith = 300.0").is_err());
    }

    #[test]
    fn a_valid_config_has_nothing_to_say() {
        assert!(toml::from_str::<Config>("theme = \"one-dark\"").is_ok());
    }
}
