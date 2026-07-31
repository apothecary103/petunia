use std::path::Path;

use gpui::prelude::*;
use gpui::{Context, Div, Entity, MouseButton, SharedString, Stateful, Window, div, px};
use gpui_component::IconName;

use super::avatar::avatar;
use super::kit;
use super::relative;
use petunia_config::Theme;
use petunia_data::{Section, Thread};
use crate::store::Store;
use crate::theme::ActivePalette;

/// The conversation list: quiet section headers, two-line entries carrying the
/// name, time and preview, and the account's identity pinned to the bottom.
pub struct Sidebar {
    store: Entity<Store>,
    /// Collapsed to avatars. The workspace owns the width and says so; the list
    /// only decides what fits.
    rail: bool,
}

impl Sidebar {
    pub fn new(store: Entity<Store>, rail: bool, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store, rail }
    }

    pub fn collapse(&mut self, rail: bool, cx: &mut Context<Self>) {
        self.rail = rail;
        cx.notify();
    }

    fn open(&mut self, thread: Thread, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.activate(thread, cx));
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let store = self.store.read(cx);
        let active = store.active().cloned();

        let translucent = store.config.sidebar.blurred();

        let Some(state) = store.state() else {
            return shell(&palette, translucent, div());
        };

        let show_preview = store.config.sidebar.show_preview;
        let rail = self.rail;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let mut list = div()
            .flex()
            .flex_col()
            .when(rail, |this| this.items_center().gap_2().px_1().pb_2())
            .when(!rail, |this| {
                this.gap_4().px(px(kit::LIST_PADDING)).pb_3()
            });

        for (at, (section, entries)) in state.index.grouped().into_iter().enumerate() {
            let (icon, label) = match &section {
                Section::Pinned => (Some(IconName::StarFill), "Pinned".to_string()),
                Section::Requests => (Some(IconName::Inbox), "Requests".to_string()),
                Section::Chats => (None, "Chats".to_string()),
                Section::Archived => (Some(IconName::FolderClosed), "Archived".to_string()),
                Section::Folder(name) => (Some(IconName::Folder), name.clone()),
            };

            let rows = entries.into_iter().map(|entry| {
                let thread = entry.thread.clone();
                let selected = active.as_ref() == Some(&thread);
                let seed = entry.thread.seed().to_vec();

                let wanted = thread.clone();

                // Who is typing, said the way the row has room for: a name in
                // the list, where the preview would be, and nothing but the dots
                // on the rail.
                let typing = state.typing(&entry.thread);
                let typing = (!typing.is_empty()).then(|| match (rail, typing.as_slice()) {
                    (true, _) => String::new(),
                    (false, [one]) => format!("{} is typing", state.name_of(*one)),
                    (false, several) => format!("{} people are typing", several.len()),
                });

                let line = Line {
                    picture: state.avatar(&entry.thread),
                    name: &entry.name,
                    seed: &seed,
                    typing,
                    // Summarised per row per frame, so not summarised at all
                    // when there is nowhere to put it.
                    preview: entry
                        .preview
                        .as_ref()
                        .filter(|_| show_preview && !rail)
                        .map(|message| message.summary()),
                    when: entry.last_activity,
                    unread: entry.unread,
                    mentions: entry.mentions,
                    muted: entry.flags.muted(now),
                    pinned: entry.flags.pinned,
                    selected,
                };

                match rail {
                    true => pip(&palette, line),
                    false => row(&palette, line),
                }
                .id(SharedString::from(format!("thread-{}", hex(&seed))))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.open(thread.clone(), cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this: &mut Self, event: &gpui::MouseDownEvent, _, cx| {
                        let thread = wanted.clone();
                        this.store
                            .update(cx, |store, cx| store.open_menu(thread, event.position, cx));
                    }),
                )
            });

            // No headings on the rail: a section label is a line of text, which
            // is the one thing the rail has no room for. A hairline says the
            // same thing in the space there is.
            if rail && at > 0 {
                list = list.child(div().w(px(24.0)).h_px().flex_none().bg(palette.border));
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .when(rail, |this| this.items_center().gap_1p5())
                    .when(!rail, |this| {
                        this.gap_0p5().child(kit::heading(icon, label, &palette))
                    })
                    .children(rows),
            );
        }

        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            // The window's traffic lights float over the top of this column, so
            // the band they sit in is its own element rather than padding on the
            // scroll area -- padding scrolls away with the content, which slid
            // the first row up underneath them.
            .child(div().h(px(super::workspace::TITLE_BAR)).flex_none())
            .child(
                div()
                    .id("conversations")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(if state.index.is_empty() && !rail {
                        div()
                            .px(px(kit::INSET))
                            .py_6()
                            .text_size(px(palette.typography.ui_size - 1.0))
                            .text_color(palette.text_muted)
                            .child("Waiting for Signal to sync your conversations…")
                            .into_any_element()
                    } else {
                        list.into_any_element()
                    }),
            );

        shell(
            &palette,
            translucent,
            div()
                .flex().flex_col()
                .size_full()
                .child(body)
                .child(identity(
                    state,
                    rail,
                    &palette,
                    cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::Settings), cx)
                    }),
                )),
        )
    }
}

fn identity(
    state: &petunia_data::State,
    rail: bool,
    palette: &Theme,
    on_settings: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let name = state.own_name();
    let picture = presence(
        avatar(
            state.avatar_for(state.aci),
            &name,
            state.aci.as_bytes(),
            30.0,
            palette,
        ),
        30.0,
        state.connection,
        palette,
    );

    // On the rail the picture is the control: there is no room for a name beside
    // it, and a gear on its own line would be the only thing in the column that
    // was not a face.
    if rail {
        return div()
            .id("identity")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .py_3()
            .border_t_1()
            .border_color(palette.border)
            .cursor_pointer()
            .tooltip(|window, cx| {
                gpui_component::tooltip::Tooltip::new("Settings").build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, on_settings)
            .child(picture)
            .into_any_element();
    }

    div()
        .flex()
        .items_center()
        .flex_none()
        .gap_2p5()
        // Lined up with the rows above rather than padded to taste, so the
        // column has one left edge.
        .px(px(kit::INSET))
        .py_3()
        // The list scrolls up to here, so there has to be something to stop at.
        .border_t_1()
        .border_color(palette.border)
        .child(picture)
        .child(
            div()
                .flex().flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(palette.typography.ui_size))
                        .font_weight(kit::EMPHASIS)
                        .text_color(palette.text)
                        .truncate()
                        .child(SharedString::from(name)),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(palette.typography.ui_size - 3.0))
                        .text_color(palette.text_muted)
                        .child(state.connection.label()),
                ),
        )
        .child(kit::icon_button(
            "settings",
            IconName::Settings,
            palette,
            on_settings,
        ))
        .into_any_element()
}

/// Whether messages are flowing, as a dot on the corner of the picture — the
/// badge every chat client draws, cut into the avatar by a ring in the colour of
/// what is behind it rather than merely sitting beside it.
fn presence(
    picture: gpui::AnyElement,
    size: f32,
    connection: petunia_data::Connection,
    palette: &Theme,
) -> Div {
    let tint = match connection {
        petunia_data::Connection::Connected => palette.success,
        petunia_data::Connection::Reconnecting => palette.warning,
        petunia_data::Connection::Connecting => palette.text_muted,
    };
    // Two fifths of the picture, which is the proportion Discord draws and the
    // smallest that reads as a state rather than as a speck, and the ring that
    // cuts it in.
    let dot = (size * 0.4).round();
    let ring = (dot / 4.0).max(2.0).round();

    div()
        .flex_none()
        .relative()
        .size(px(size))
        .child(picture)
        .child(
            div()
                .absolute()
                // In the corner of the picture's box, which for a circle puts
                // the dot's centre inside the disc: the ring is then what eats
                // into it rather than the colour.
                .bottom_0()
                .right_0()
                .size(px(dot))
                .rounded_full()
                .bg(tint)
                .border(px(ring))
                .border_color(palette.surface),
        )
}

fn shell(palette: &Theme, translucent: bool, body: Div) -> Div {
    // Mostly solid. The vibrancy layer underneath is not there to be looked
    // through -- at any strength where the wallpaper is legible the list stops
    // being -- but to give the surface the depth a flat fill does not have.
    // At 0.97 there was nothing to see; below about 0.9 the frost reads.
    let backing = match translucent {
        true => gpui::Hsla {
            a: 0.84,
            ..palette.surface
        },
        false => palette.surface,
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(backing)
        .text_color(palette.text)
        .child(body)
}

struct Line<'a> {
    picture: Option<&'a Path>,
    name: &'a str,
    seed: &'a [u8],
    /// Who is typing, when anybody is. It replaces the preview rather than
    /// joining it: the preview is what was last said, and this is what is being
    /// said now.
    typing: Option<String>,
    preview: Option<String>,
    when: u64,
    unread: u32,
    mentions: u32,
    muted: bool,
    pinned: bool,
    selected: bool,
}

/// One conversation on the collapsed rail: the picture, and the two things worth
/// knowing without a line of text. Nothing that would be truncated to nothing.
fn pip(palette: &Theme, line: Line<'_>) -> Stateful<Div> {
    const EDGE: f32 = 38.0;

    let tint = if line.mentions > 0 && !line.muted {
        palette.accent
    } else if line.muted {
        palette.text_muted
    } else {
        palette.text_dim
    };

    div()
        .id("pip")
        .flex_none()
        .relative()
        .size(px(EDGE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        // The ring is the whole of what says "this is the one you are in": there
        // is no name here to embolden and no room for a fill that would not
        // simply cover the picture.
        .border_2()
        .border_color(match line.selected {
            true => palette.accent,
            false => gpui::transparent_black(),
        })
        .child(avatar(
            line.picture,
            line.name,
            line.seed,
            EDGE - 8.0,
            palette,
        ))
        .tooltip({
            let name = SharedString::from(line.name.to_owned());
            move |window, cx| {
                gpui_component::tooltip::Tooltip::new(name.clone()).build(window, cx)
            }
        })
        .when(line.unread > 0, |this| {
            this.child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .size(px(9.0))
                    .rounded_full()
                    .bg(tint)
                    .border_2()
                    .border_color(palette.surface),
            )
        })
        // The dots go where a presence badge goes, which is the corner the eye
        // already looks at for "what is this person doing".
        .when(line.typing.is_some(), |this| {
            this.child(
                div()
                    .absolute()
                    .bottom(px(-2.0))
                    .right(px(-4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_1()
                    .py(px(3.0))
                    .rounded_full()
                    .bg(palette.elevated)
                    .border_2()
                    .border_color(palette.surface)
                    .child(kit::typing(
                        3.0,
                        palette.text_dim,
                        format!("typing-pip-{}", hex(line.seed)),
                    )),
            )
        })
}

fn row(palette: &Theme, line: Line<'_>) -> Stateful<Div> {
    let unread = line.unread > 0;
    let title_color = if line.muted {
        palette.text_dim
    } else if unread || line.selected {
        palette.text
    } else {
        palette.text_dim
    };

    kit::row("row", line.selected, palette)
        .child(
            div()
                .pt_0p5()
                .child(avatar(line.picture, line.name, line.seed, 30.0, palette)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    // Centred, not baseline-aligned: a glyph has no baseline to
                    // share with text, and lining one up by it hangs it below.
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(palette.typography.ui_size))
                                .when(unread && !line.muted, |this| {
                                    this.font_weight(kit::EMPHASIS)
                                })
                                .text_color(title_color)
                                .child(SharedString::from(line.name.to_owned())),
                        )
                        .when(line.pinned, |this| {
                            this.child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .child(kit::icon(
                                        IconName::StarFill,
                                        11.0,
                                        palette.text_muted,
                                    )),
                            )
                        })
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(palette.typography.ui_size - 3.0))
                                .text_color(palette.text_muted)
                                .child(SharedString::from(relative::short(line.when))),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .when_some(line.typing.clone(), |this, who| {
                            this.child(kit::typing(
                                4.0,
                                palette.text_dim,
                                format!("typing-{}", hex(line.seed)),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(palette.typography.ui_size - 2.0))
                                    .text_color(palette.text_dim)
                                    .child(SharedString::from(who)),
                            )
                        })
                        .when(line.typing.is_none(), |this| {
                            this.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(palette.typography.ui_size - 2.0))
                                    .text_color(if unread {
                                        palette.text_dim
                                    } else {
                                        palette.text_muted
                                    })
                                    .child(SharedString::from(
                                        line.preview.clone().unwrap_or_default(),
                                    )),
                            )
                        })
                        .when(unread, |this| {
                            this.child(badge(line.unread, line.mentions, line.muted, palette))
                        }),
                ),
        )
}

/// A dot when there is simply something new, a count once it is worth counting,
/// and the accent reserved for a mention. A muted conversation still counts, but
/// dimly: Signal's own behaviour, and the difference between "there is something
/// here" and "look at this".
fn badge(unread: u32, mentions: u32, muted: bool, palette: &Theme) -> Div {
    let mentioned = mentions > 0 && !muted;
    let tint = if mentioned {
        palette.accent
    } else if muted {
        palette.text_muted
    } else {
        palette.text_dim
    };

    if unread == 1 && !mentioned {
        return kit::dot(tint);
    }
    kit::chip(unread.to_string(), tint, palette)
}

/// One allocation, not one per byte. The list is a scrolling `div`, so every row
/// is rebuilt on every frame of every flick, and a group's seed is 32 bytes.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
