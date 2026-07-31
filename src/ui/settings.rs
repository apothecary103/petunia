//! Every preference, in a window.
//!
//! It edits `config.toml` rather than keeping a second copy of the truth: a
//! change is written, the watcher notices, and the whole app reloads exactly as
//! if the file had been edited by hand. That means there is no path by which the
//! window and the file can disagree, and no reload logic that exists only for
//! settings.

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, SharedString, Window, div, px};
use gpui_component::IconName;

use super::kit;
use crate::config::keys::Preset;
use crate::config::messages::{Density, Timestamps};
use crate::config::{Config, GroupNotifications, Sort, theme, write};
use crate::store::Store;
use crate::theme::ActivePalette;

pub struct Dismissed;

/// The whole file was asked for. The workspace opens the editor, because it
/// covers more than this sheet does.
pub struct EditFile;

impl gpui::EventEmitter<Dismissed> for Settings {}
impl gpui::EventEmitter<EditFile> for Settings {}

pub struct Settings {
    store: Entity<Store>,
    /// What the file will say. Edited here and written on every change, so the
    /// window never holds an opinion the file does not.
    draft: Config,
    /// Reported rather than swallowed: a settings window that silently fails to
    /// save is worse than one that will not open.
    failed: Option<String>,
    focus: gpui::FocusHandle,
}

impl Settings {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let draft = (*store.read(cx).config).clone();
        Self {
            store,
            draft,
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
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
    fn header(&self, palette: &crate::config::Theme, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(
                div()
                    .id("edit-file")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .py_1()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(|this| this.bg(palette.hover))
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text_muted)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.emit(EditFile)),
                    )
                    .child(kit::icon(IconName::File, 13.0, palette.text_muted))
                    .child("Edit config.toml"),
            )
            .child(kit::icon_button(
                "close-settings",
                IconName::Close,
                palette,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            ))
    }

    fn appearance(
        &self,
        draft: &Config,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let themes = theme::available();
        let chosen = draft.theme.clone();

        section("Appearance", palette).child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    field("Theme", palette).child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1p5()
                            .justify_end()
                            .children(themes.into_iter().map(|name| {
                                let selected = name == chosen;
                                let wanted = name.clone();
                                chip(
                                    name.clone(),
                                    selected,
                                    palette,
                                    cx.listener(move |this: &mut Self, _, _, cx| {
                                        let wanted = wanted.clone();
                                        this.change(|config| config.theme = wanted, cx);
                                    }),
                                )
                            })),
                    ),
                )
                .child(
                    field("Scale", palette).child(stepper(
                        format!("{:.2}", draft.scale),
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| config.scale = (config.scale - 0.05).max(0.5),
                                cx,
                            )
                        }),
                        cx.listener(|this: &mut Self, _, _, cx| {
                            this.change(
                                |config| config.scale = (config.scale + 0.05).min(3.0),
                                cx,
                            )
                        }),
                    )),
                ),
        )
    }

    fn messages(
        &self,
        draft: &Config,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        section("Messages", palette)
            .child(field("Density", palette).child(choices(
                [("Comfortable", Density::Comfortable), ("Compact", Density::Compact)],
                draft.messages.density,
                palette,
                cx,
                |config, density| config.messages.density = density,
            )))
            .child(field("Timestamps", palette).child(choices(
                [
                    ("Always", Timestamps::Always),
                    ("On hover", Timestamps::Hover),
                    ("Never", Timestamps::Never),
                ],
                draft.messages.timestamps,
                palette,
                cx,
                |config, timestamps| config.messages.timestamps = timestamps,
            )))
            .child(field("Attribute your messages by name", palette).child(toggle(
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
            )))
            .child(field("Day separators", palette).child(toggle(
                draft.messages.date_separators,
                palette,
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| {
                            config.messages.date_separators = !config.messages.date_separators
                        },
                        cx,
                    )
                }),
            )))
            .child(
                field("Group messages sent within", palette).child(stepper(
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
                )),
            )
    }

    fn list(
        &self,
        draft: &Config,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        section("Conversation list", palette)
            .child(field("Sort by", palette).child(choices(
                [("Recent", Sort::Recent), ("Name", Sort::Name)],
                draft.sidebar.sort,
                palette,
                cx,
                |config, sort| config.sidebar.sort = sort,
            )))
            .child(field("Show a preview", palette).child(toggle(
                draft.sidebar.show_preview,
                palette,
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| config.sidebar.show_preview = !config.sidebar.show_preview,
                        cx,
                    )
                }),
            )))
            .when(cfg!(target_os = "macos"), |this| {
                this.child(field("Blur the desktop behind it", palette).child(toggle(
                    draft.sidebar.translucent,
                    palette,
                    cx.listener(|this: &mut Self, _, _, cx| {
                        this.change(
                            |config| config.sidebar.translucent = !config.sidebar.translucent,
                            cx,
                        )
                    }),
                )))
            })
            .child(
                div()
                    .pb_2()
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text_muted)
                    .child(match cfg!(target_os = "macos") {
                        true => "Takes effect on restart: the blur is a property of the window.",
                        false => "",
                    }),
            )
            .child(field("Width", palette).child(stepper(
                format!("{:.0}", draft.sidebar.width),
                palette,
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| config.sidebar.width = (config.sidebar.width - 20.0).max(180.0),
                        cx,
                    )
                }),
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| config.sidebar.width = (config.sidebar.width + 20.0).min(480.0),
                        cx,
                    )
                }),
            )))
    }

    fn media(
        &self,
        draft: &Config,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        section("Media", palette)
            .child(
                div()
                    .pb_2()
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text_muted)
                    .child(
                        "Signal's servers drop attachments after a few weeks. Anything \
                         not fetched while it is fresh is gone for good.",
                    ),
            )
            .child(field("Download pictures", palette).child(toggle(
                draft.media.auto_download_images,
                palette,
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| {
                            config.media.auto_download_images = !config.media.auto_download_images
                        },
                        cx,
                    )
                }),
            )))
            .child(field("Download audio", palette).child(toggle(
                draft.media.auto_download_audio,
                palette,
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| {
                            config.media.auto_download_audio = !config.media.auto_download_audio
                        },
                        cx,
                    )
                }),
            )))
            .child(field("Download video", palette).child(toggle(
                draft.media.auto_download_video,
                palette,
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| {
                            config.media.auto_download_video = !config.media.auto_download_video
                        },
                        cx,
                    )
                }),
            )))
            .child(field("Only below", palette).child(stepper(
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
            )))
            .child(field("Keep cached", palette).child(stepper(
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
                            config.media.cache_limit = (config.media.cache_limit + 256).min(65_536)
                        },
                        cx,
                    )
                }),
            )))
    }

    fn notifications(
        &self,
        draft: &Config,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        section("Notifications", palette)
            .child(
                div()
                    .pb_2()
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text_muted)
                    .child("Not built yet; these are recorded and nothing reads them."),
            )
            .child(field("Enabled", palette).child(toggle(
                draft.notifications.enabled,
                palette,
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.change(
                        |config| config.notifications.enabled = !config.notifications.enabled,
                        cx,
                    )
                }),
            )))
            .child(field("Show what was said", palette).child(toggle(
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
            )))
            .child(field("For groups", palette).child(choices(
                [
                    ("Everything", GroupNotifications::All),
                    ("Mentions", GroupNotifications::Mentions),
                    ("Nothing", GroupNotifications::None),
                ],
                draft.notifications.groups,
                palette,
                cx,
                |config, groups| config.notifications.groups = groups,
            )))
    }

    fn keys(
        &self,
        draft: &Config,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let current = draft.keys.matches();

        section("Keyboard", palette)
            .child(field("Preset", palette).child(
                div().flex().gap_1p5().children(Preset::every().map(|preset| {
                    chip(
                        preset.label(),
                        current == Some(preset),
                        palette,
                        cx.listener(move |this: &mut Self, _, _, cx| {
                            this.change(
                                |config| {
                                    config.keys = crate::config::Keys::preset(preset);
                                },
                                cx,
                            )
                        }),
                    )
                })),
            ))
            .child(
                div()
                    .pt_1()
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text_muted)
                    .child(match current {
                        Some(_) => "Press cmd+/ to see the bindings. Individual keys can be \
                                    rebound by editing config.toml.",
                        None => "These bindings have been edited, so no preset matches. Press \
                                 cmd+/ to see them.",
                    }),
            )
    }
}

fn section(title: &'static str, palette: &crate::config::Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .pt_5()
        .child(
            div()
                .pb_2()
                .text_size(px(palette.typography.ui_size - 2.0))
                .text_color(palette.text_muted)
                .child(title),
        )
}

/// One preference: what it is on the left, what it is set to on the right.
fn field(label: &'static str, palette: &crate::config::Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .py_2()
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .flex_none()
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text_dim)
                .child(label),
        )
}

/// A row of mutually exclusive choices, which is every enum in the config.
fn choices<T: PartialEq + Copy + 'static, const N: usize>(
    options: [(&'static str, T); N],
    chosen: T,
    palette: &crate::config::Theme,
    cx: &mut Context<Settings>,
    set: impl Fn(&mut Config, T) + Copy + 'static,
) -> gpui::Div {
    div()
        .flex()
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

fn chip(
    label: impl Into<SharedString>,
    selected: bool,
    palette: &crate::config::Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
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
        .border_color(if selected {
            palette.accent
        } else {
            palette.border
        })
        .when(selected, |this| this.bg(kit::tinted(palette.accent)))
        .when(!selected, |this| this.hover(|this| this.bg(palette.hover)))
        .text_size(px(palette.typography.ui_size - 1.0))
        .text_color(if selected {
            palette.text
        } else {
            palette.text_dim
        })
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label)
}

fn toggle(
    on: bool,
    palette: &crate::config::Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
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
    palette: &crate::config::Theme,
    less: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
    more: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Div {
    let value = value.into();

    div()
        .flex()
        .items_center()
        .gap_1()
        .child(kit::icon_button(
            SharedString::from(format!("less-{value}")),
            IconName::Minus,
            palette,
            less,
        ))
        .child(
            div()
                .min_w(px(64.0))
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text)
                .child(value.clone()),
        )
        .child(kit::icon_button(
            SharedString::from(format!("more-{value}")),
            IconName::Plus,
            palette,
            more,
        ))
}
