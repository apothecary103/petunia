//! Every preference, in a window.
//!
//! It edits `config.toml` rather than keeping a second copy of the truth: a
//! change is written, the watcher notices, and the whole app reloads exactly as
//! if the file had been edited by hand. That means there is no path by which the
//! window and the file can disagree, and no reload logic that exists only for
//! settings.

use gpui::prelude::*;
use gpui::{App, Context, Entity, MouseButton, MouseDownEvent, SharedString, Window, div, px};

use super::kit;
use petunia_config::keys::Preset;
use petunia_config::messages::{Density, Layout, Timestamps};
use petunia_config::{Config, GroupNotifications, Sort, Theme, theme, write};
use crate::store::Store;
use crate::theme::ActivePalette;

pub struct Dismissed;

/// The whole file was asked for. The workspace opens the editor, because it
/// covers more than this sheet does.
pub struct EditFile;

impl gpui::EventEmitter<Dismissed> for Settings {}
impl gpui::EventEmitter<EditFile> for Settings {}

/// What clicking something does. Boxed rather than generic because a select is
/// built from a list of them and they are all different closures.
type Click = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;

/// What one row of a select offers, and what picking it does.
type Option_ = (SharedString, bool, Click);

pub struct Settings {
    store: Entity<Store>,
    /// What the file will say. Edited here and written on every change, so the
    /// window never holds an opinion the file does not.
    draft: Config,
    /// Which select is open, if any. One at a time, because two lists of options
    /// covering each other is not a choice anybody is making.
    open: Option<&'static str>,
    /// Reported rather than swallowed: a settings window that silently fails to
    /// save is worse than one that will not open.
    failed: Option<String>,
    focus: gpui::FocusHandle,
}

impl Settings {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        // The file is the truth, and it can change from under this window --
        // through the theme picker, through a hand edit, through the watcher. A
        // draft that did not follow would put whatever it was holding back the
        // next time anything here was touched.
        cx.observe(&store, |this: &mut Self, store, cx| {
            let config = store.read(cx).config.clone();
            if *config != this.draft {
                this.draft = (*config).clone();
                cx.notify();
            }
        })
        .detach();

        let draft = (*store.read(cx).config).clone();
        Self {
            store,
            draft,
            open: None,
            failed: None,
            focus: cx.focus_handle(),
        }
    }

    /// Applies a change and writes it. The watcher reloads the file, which is
    /// what actually makes the change take effect -- so there is exactly one
    /// path from a preference to the running app.
    fn change(&mut self, edit: impl FnOnce(&mut Config), cx: &mut Context<Self>) {
        edit(&mut self.draft);
        self.failed = match write::save(&self.draft) {
            Ok(()) => None,
            Err(error) => Some(format!("could not write config.toml: {error}")),
        };
        // Applied here as well as through the watcher: a filesystem that does
        // not report changes would otherwise leave the window inert.
        let config = std::sync::Arc::new(self.draft.clone());
        self.store
            .update(cx, |store, cx| store.config_changed(config, cx));
        cx.notify();
    }

    /// Opens or closes one of the selects.
    fn toggle(&mut self, which: &'static str, cx: &mut Context<Self>) {
        self.open = match self.open {
            Some(open) if open == which => None,
            _ => Some(which),
        };
        cx.notify();
    }

    /// Picking a theme takes effect at once rather than waiting for the watcher,
    /// because the whole point of choosing one is looking at it.
    fn pick_theme(&mut self, name: String, cx: &mut Context<Self>) {
        self.open = None;
        let wanted = name.clone();
        self.change(|config| config.theme = wanted, cx);
        crate::theme::install(theme::load(&name).0, cx);
    }
}

impl gpui::Focusable for Settings {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Settings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let draft = self.draft.clone();

        div()
            .id("settings")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Hsla {
                a: 0.66,
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
                    .w(px(620.0))
                    .max_h(px(680.0))
                    .flex()
                    .flex_col()
                    .rounded(px(kit::RADIUS_LG))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .overflow_hidden()
                    // A click in the sheet neither dismisses it nor leaves a
                    // select hanging open behind whatever was clicked.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            cx.stop_propagation();
                            if this.open.take().is_some() {
                                cx.notify();
                            }
                        }),
                    )
                    .child(self.header(&palette, cx))
                    .child(
                        div()
                            .id("settings-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px_5()
                            .pb_5()
                            .child(self.appearance(&draft, &palette, cx))
                            .child(self.messages(&draft, &palette, cx))
                            .child(self.list(&draft, &palette, cx))
                            .child(self.media(&draft, &palette, cx))
                            .child(self.notifications(&draft, &palette, cx))
                            .child(self.keys(&draft, &palette, cx)),
                    )
                    .when_some(self.failed.clone(), |this, failed| {
                        this.child(
                            div()
                                .px_5()
                                .py_3()
                                .border_t_1()
                                .border_color(palette.border)
                                .text_size(px(palette.typography.ui_size - 1.0))
                                .text_color(palette.danger)
                                .child(SharedString::from(failed)),
                        )
                    }),
            )
    }
}

impl Settings {
    fn header(&self, palette: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_5()
            .py_4()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex_1()
                    .text_size(px(palette.typography.ui_size + 3.0))
                    .text_color(palette.text)
                    .child("Settings"),
            )
            // No icon. The one that was here was a document glyph on a control
            // whose label already says which document, and it read as a file
            // picker.
            .child(
                div()
                    .id("edit-file")
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(|this| this.bg(palette.hover))
                    .text_size(px(palette.typography.ui_size - 1.0))
                    .text_color(palette.text_dim)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.emit(EditFile)),
                    )
                    .child("Edit config.toml"),
            )
            .child(kit::icon_button(
                "close-settings",
                gpui_component::IconName::Close,
                palette,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            ))
    }

    fn appearance(
        &self,
        draft: &Config,
        palette: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chosen = match draft.theme.is_empty() {
            true => "dark".to_owned(),
            false => draft.theme.clone(),
        };
        let options: Vec<Option_> = theme::available()
            .into_iter()
            .map(|name| {
                let selected = name == chosen;
                let wanted = name.clone();
                let pick: Click = Box::new(cx.listener(move |this: &mut Self, _, _, cx| {
                    this.pick_theme(wanted.clone(), cx)
                }));
                (SharedString::from(name), selected, pick)
            })
            .collect();

        group(
            "Appearance",
            palette,
            vec![
                field("Theme", palette)
                    .child(select(
                        "theme",
                        SharedString::from(chosen),
                        self.open == Some("theme"),
                        options,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| this.toggle("theme", cx)),
                    ))
                    .into_any_element(),
                field("Scale", palette)
                    .child(stepper(
                        format!("{:.2}", draft.scale),
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(|config| config.scale = (config.scale - 0.05).max(0.5), cx)
                        }),
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(|config| config.scale = (config.scale + 0.05).min(3.0), cx)
                        }),
                    ))
                    .into_any_element(),
            ],
            None,
        )
    }

    fn messages(
        &self,
        draft: &Config,
        palette: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let layouts = Layout::every().map(|layout| (layout.label(), layout));

        group(
            "Messages",
            palette,
            vec![
                described("Layout", draft.messages.layout.describe(), palette)
                    .child(choices(
                        layouts,
                        draft.messages.layout,
                        palette,
                        cx,
                        |config, layout| config.messages.layout = layout,
                    ))
                    .into_any_element(),
                field("Density", palette)
                    .child(choices(
                        [
                            ("Comfortable", Density::Comfortable),
                            ("Compact", Density::Compact),
                        ],
                        draft.messages.density,
                        palette,
                        cx,
                        |config, density| config.messages.density = density,
                    ))
                    .into_any_element(),
                field("Timestamps", palette)
                    .child(choices(
                        [
                            ("Always", Timestamps::Always),
                            ("On hover", Timestamps::Hover),
                            ("Never", Timestamps::Never),
                        ],
                        draft.messages.timestamps,
                        palette,
                        cx,
                        |config, timestamps| config.messages.timestamps = timestamps,
                    ))
                    .into_any_element(),
                field("Group messages sent within", palette)
                    .child(stepper(
                        format!("{}s", draft.messages.group_within),
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.messages.group_within =
                                        config.messages.group_within.saturating_sub(60).max(30)
                                },
                                cx,
                            )
                        }),
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.messages.group_within =
                                        (config.messages.group_within + 60).min(3_600)
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("Day separators", palette)
                    .child(toggle(
                        draft.messages.date_separators,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.messages.date_separators =
                                        !config.messages.date_separators
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("Attribute your messages by name", palette)
                    .child(toggle(
                        draft.messages.show_own_name,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.messages.show_own_name = !config.messages.show_own_name
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
            ],
            None,
        )
    }

    fn list(&self, draft: &Config, palette: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = vec![
            field("Sort by", palette)
                .child(choices(
                    [("Recent", Sort::Recent), ("Name", Sort::Name)],
                    draft.sidebar.sort,
                    palette,
                    cx,
                    |config, sort| config.sidebar.sort = sort,
                ))
                .into_any_element(),
            field("Show a preview", palette)
                .child(toggle(
                    draft.sidebar.show_preview,
                    palette,
                    cx.listener(|this: &mut Self, _, _, cx| {
                        this.change(
                            |config| config.sidebar.show_preview = !config.sidebar.show_preview,
                            cx,
                        )
                    }),
                ))
                .into_any_element(),
        ];
        // No width here. It is what dragging the list's right edge sets, and a
        // stepper for the same number is a second control for one preference --
        // which is a promise that they cannot disagree.

        if cfg!(target_os = "macos") {
            rows.push(
                described(
                    "Blur the desktop behind it",
                    "Takes effect on restart: the blur is a property of the window.",
                    palette,
                )
                .child(toggle(
                    draft.sidebar.translucent,
                    palette,
                    cx.listener(|this: &mut Self, _, _, cx| {
                        this.change(
                            |config| config.sidebar.translucent = !config.sidebar.translucent,
                            cx,
                        )
                    }),
                ))
                .into_any_element(),
            );
        }

        group("Conversation list", palette, rows, None)
    }

    fn media(&self, draft: &Config, palette: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        group(
            "Media",
            palette,
            vec![
                field("Download pictures", palette)
                    .child(toggle(
                        draft.media.auto_download_images,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.media.auto_download_images =
                                        !config.media.auto_download_images
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("Download audio", palette)
                    .child(toggle(
                        draft.media.auto_download_audio,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.media.auto_download_audio =
                                        !config.media.auto_download_audio
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("Download video", palette)
                    .child(toggle(
                        draft.media.auto_download_video,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.media.auto_download_video =
                                        !config.media.auto_download_video
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("Only below", palette)
                    .child(stepper(
                        format!("{} MB", draft.media.auto_download_limit),
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.media.auto_download_limit =
                                        config.media.auto_download_limit.saturating_sub(2).max(1)
                                },
                                cx,
                            )
                        }),
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.media.auto_download_limit =
                                        (config.media.auto_download_limit + 2).min(256)
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("Keep cached", palette)
                    .child(stepper(
                        format!("{} MB", draft.media.cache_limit),
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.media.cache_limit =
                                        config.media.cache_limit.saturating_sub(256).max(256)
                                },
                                cx,
                            )
                        }),
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.media.cache_limit =
                                        (config.media.cache_limit + 256).min(65_536)
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
            ],
            Some(
                "Signal's servers drop attachments after a few weeks. Anything not \
                 fetched while it is fresh is gone for good.",
            ),
        )
    }

    fn notifications(
        &self,
        draft: &Config,
        palette: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        group(
            "Notifications",
            palette,
            vec![
                field("Enabled", palette)
                    .child(toggle(
                        draft.notifications.enabled,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.notifications.enabled = !config.notifications.enabled
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("Show what was said", palette)
                    .child(toggle(
                        draft.notifications.show_content,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.notifications.show_content =
                                        !config.notifications.show_content
                                },
                                cx,
                            )
                        }),
                    ))
                    .into_any_element(),
                field("For groups", palette)
                    .child(choices(
                        [
                            ("Everything", GroupNotifications::All),
                            ("Mentions", GroupNotifications::Mentions),
                            ("Nothing", GroupNotifications::None),
                        ],
                        draft.notifications.groups,
                        palette,
                        cx,
                        |config, groups| config.notifications.groups = groups,
                    ))
                    .into_any_element(),
            ],
            Some("Not built yet; these are recorded and nothing reads them."),
        )
    }

    fn keys(&self, draft: &Config, palette: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let current = draft.keys.matches();

        group(
            "Keyboard",
            palette,
            vec![
                field("Preset", palette)
                    .child(div().flex().gap_1p5().children(Preset::every().map(
                        |preset| {
                            chip(
                                preset.label(),
                                current == Some(preset),
                                palette,
                                cx.listener(move |this: &mut Self, _, _, cx| {
                                    this.change(
                                        |config| config.keys = petunia_config::Keys::preset(preset),
                                        cx,
                                    )
                                }),
                            )
                        },
                    )))
                    .into_any_element(),
            ],
            Some(match current {
                Some(_) => "Press cmd+/ to see the bindings. Individual keys can be rebound \
                            by editing config.toml.",
                None => "These bindings have been edited, so no preset matches. Press cmd+/ \
                         to see them.",
            }),
        )
    }
}

/// One section: a quiet heading, a card of rows, and an optional footnote.
///
/// A card rather than a run of rows each with a rule under it. The old shape left
/// every section's last rule floating between it and the next heading, which is
/// what made the window read as one undifferentiated list.
fn group(
    title: &'static str,
    palette: &Theme,
    rows: Vec<gpui::AnyElement>,
    footnote: Option<&'static str>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .pt_5()
        .child(kit::section(title, palette))
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(kit::RADIUS))
                .border_1()
                .border_color(palette.border)
                .children(rows.into_iter().enumerate().map(|(at, row)| {
                    div()
                        .px_3()
                        .py_2p5()
                        .when(at > 0, |this| {
                            this.border_t_1().border_color(palette.border)
                        })
                        .child(row)
                })),
        )
        .when_some(footnote, |this, footnote| {
            this.child(
                div()
                    .pt_2()
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text_muted)
                    .child(footnote),
            )
        })
}

/// One preference: what it is on the left, what it is set to on the right.
fn field(label: &'static str, palette: &Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_none()
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text_dim)
                .child(label),
        )
}

/// A preference that needs a line of explanation. The explanation goes under the
/// label rather than under the row, so it stays attached to what it explains.
fn described(label: &'static str, note: &'static str, palette: &Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_size(px(palette.typography.ui_size))
                        .text_color(palette.text_dim)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(palette.typography.ui_size - 2.0))
                        .text_color(palette.text_muted)
                        .child(note),
                ),
        )
}

/// A row of mutually exclusive choices, which is every small enum in the config.
fn choices<T: PartialEq + Copy + 'static, const N: usize>(
    options: [(&'static str, T); N],
    chosen: T,
    palette: &Theme,
    cx: &mut Context<Settings>,
    set: impl Fn(&mut Config, T) + Copy + 'static,
) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .gap_1p5()
        .children(options.map(|(label, value)| {
            chip(
                label,
                value == chosen,
                palette,
                cx.listener(move |this: &mut Settings, _, _, cx| {
                    this.change(|config| set(config, value), cx)
                }),
            )
        }))
}

/// One of many, shown as what is chosen rather than as all of them. Thirteen
/// themes as a wrapped row of chips was most of the window and read as a colour
/// swatch grid.
fn select(
    id: &'static str,
    chosen: SharedString,
    open: bool,
    options: Vec<Option_>,
    palette: &Theme,
    toggle: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    let rows = options.into_iter().enumerate().map(|(at, (label, selected, pick))| {
        div()
            .id(SharedString::from(format!("{id}-option-{at}")))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(5.0))
            .cursor_pointer()
            .hover(|this| this.bg(palette.hover))
            .text_size(px(palette.typography.ui_size))
            .text_color(match selected {
                true => palette.text,
                false => palette.text_dim,
            })
            .child(div().flex_1().min_w_0().truncate().child(label))
            .when(selected, |this| {
                this.child(kit::icon(
                    gpui_component::IconName::Check,
                    13.0,
                    palette.accent,
                ))
            })
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                pick(event, window, cx);
                cx.stop_propagation();
            })
    });

    div()
        .flex_none()
        .relative()
        .child(
            div()
                .id(SharedString::from(format!("{id}-select")))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .w(px(190.0))
                .px_2p5()
                .py_1()
                .rounded(px(6.0))
                .cursor_pointer()
                .bg(palette.sunken)
                .border_1()
                .border_color(match open {
                    true => palette.border_focus,
                    false => palette.border,
                })
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text)
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    toggle(event, window, cx);
                    // Otherwise the sheet's own handler, which closes whatever is
                    // open, would close this in the same click that opened it.
                    cx.stop_propagation();
                })
                .child(div().flex_1().min_w_0().truncate().child(chosen))
                .child(kit::icon(
                    gpui_component::IconName::ChevronDown,
                    13.0,
                    palette.text_muted,
                )),
        )
        .when(open, |this| {
            // Deferred, so it paints after every one of its ancestors and
            // outside their content masks. Drawn in place it was both painted
            // over by the rows below it -- which is what made it look
            // transparent -- and clipped by the scroll area it sits in.
            this.child(gpui::deferred(
                div()
                    .id(SharedString::from(format!("{id}-options")))
                    .absolute()
                    .top(px(30.0))
                    .right_0()
                    .w(px(190.0))
                    .max_h(px(240.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .p_1()
                    .rounded(px(kit::RADIUS))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .shadow_lg()
                    .children(rows),
            ))
        })
}

fn chip(
    label: impl Into<SharedString>,
    selected: bool,
    palette: &Theme,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();

    div()
        .id(SharedString::from(format!("choice-{label}")))
        .flex_none()
        .px_2p5()
        .py_1()
        .rounded(px(6.0))
        .cursor_pointer()
        .border_1()
        .border_color(match selected {
            true => palette.accent,
            false => palette.border,
        })
        .when(selected, |this| this.bg(kit::tinted(palette.accent)))
        .when(!selected, |this| this.hover(|this| this.bg(palette.hover)))
        .text_size(px(palette.typography.ui_size - 1.0))
        .text_color(match selected {
            true => palette.text,
            false => palette.text_dim,
        })
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label)
}

fn toggle(
    on: bool,
    palette: &Theme,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id("toggle")
        .flex_none()
        .w(px(38.0))
        .h(px(22.0))
        .p(px(2.0))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { palette.accent } else { palette.sunken })
        .border_1()
        .border_color(if on { palette.accent } else { palette.border })
        .flex()
        .items_center()
        .when(on, |this| this.justify_end())
        .on_mouse_down(MouseButton::Left, on_click)
        .child(
            div()
                .size(px(16.0))
                .rounded_full()
                .bg(if on { palette.on_accent } else { palette.text_muted }),
        )
}

fn stepper(
    value: impl Into<SharedString>,
    palette: &Theme,
    less: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    more: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    let value = value.into();

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .child(kit::icon_button(
            SharedString::from(format!("less-{value}")),
            gpui_component::IconName::Minus,
            palette,
            less,
        ))
        .child(
            // Fixed and centred, so stepping from "9 MB" to "10 MB" does not move
            // the buttons either side of it.
            div()
                .w(px(72.0))
                .text_center()
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text)
                .child(value.clone()),
        )
        .child(kit::icon_button(
            SharedString::from(format!("more-{value}")),
            gpui_component::IconName::Plus,
            palette,
            more,
        ))
}
