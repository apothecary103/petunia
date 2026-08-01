use std::path::Path;

use gpui::prelude::*;
use gpui::{Context, Div, Entity, Hsla, MouseButton, SharedString, Stateful, Window, div, px};
use gpui_component::IconName;

use super::avatar::avatar;
use super::kit;
use super::relative;
use petunia_config::Theme;
use petunia_data::{Section, Thread, hex};
use crate::store::Store;
use crate::theme::ActivePalette;

/// How large a face is in the list. One value for the rows and the identity, so
/// the column has one rhythm rather than two.
const FACE: f32 = 34.0;

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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        // macOS hides the traffic lights in fullscreen, and no other platform
        // ever draws them here.
        let lights = cfg!(target_os = "macos") && !window.is_fullscreen();
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
                let seed = entry.thread.seed();

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
                    seed,
                    typing,
                    preview: entry
                        .preview
                        .as_ref()
                        .filter(|_| show_preview && !rail)
                        .map(|preview| preview.line.as_str()),
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
                .id(SharedString::from(format!("thread-{}", hex(seed))))
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
            // the first row up underneath them. The band exists *for* the lights:
            // where there are none it is only what the compose button needs. Off
            // macOS there never are, and in fullscreen macOS takes them away, so
            // a band that stayed put there was forty pixels of nothing at the top
            // of the column.
            //
            // Which the rail cannot share with them: it is no wider than a face,
            // and the lights reach most of the way across it -- so a button at
            // its right edge is a button underneath the one that maximises the
            // window. There the band is left to them and the compose control
            // takes the row below, centred like everything else on the rail.
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .when(rail, |this| this.justify_center())
                    .when(!rail, |this| {
                        this.justify_end().px(px(kit::LIST_PADDING))
                    })
                    .when(lights && !rail, |this| {
                        this.h(px(super::workspace::TITLE_BAR))
                    })
                    .when(lights && rail, |this| {
                        this.mt(px(super::workspace::TITLE_BAR))
                    })
                    .when(!lights, |this| this.pt_2())
                    .child(kit::icon_button(
                        "new-chat",
                        IconName::Plus,
                        &palette,
                        cx.listener(|_, _, window, cx| {
                            window.dispatch_action(Box::new(crate::actions::NewChat), cx)
                        }),
                    )),
            )
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
            FACE,
            palette,
        ),
        FACE,
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
        .id("identity")
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

/// Whether messages are flowing, as a badge on the corner of the picture.
///
/// Discord's own geometry, to the proportion: the dot is five sixteenths of the
/// picture, its edge flush with the bottom-right of the box, and the gap around
/// it is a hole punched in the avatar rather than a ring drawn beside it. Its
/// *shape* is the state as much as its colour is -- a disc while messages flow,
/// a crescent while the connection is coming back, a hollow ring before it is up
/// -- so the three are told apart by more than a hue.
fn presence(
    picture: gpui::AnyElement,
    size: f32,
    connection: petunia_data::Connection,
    palette: &Theme,
) -> Div {
    /// Discord's ten pixels of dot and two of gap, on a thirty-two pixel avatar.
    const DOT: f32 = 10.0 / 32.0;
    const GAP: f32 = 2.0 / 32.0;

    let tint = match connection {
        petunia_data::Connection::Connected => palette.success,
        petunia_data::Connection::Reconnecting => palette.warning,
        petunia_data::Connection::Connecting => palette.text_muted,
    };
    let dot = (size * DOT).round().max(8.0);
    let gap = (size * GAP).round().max(1.0);

    div()
        .flex_none()
        .relative()
        .size(px(size))
        .child(picture)
        .child(
            // The hole, which is the picture's own colour: the dot sits centred
            // in it, so its edge lands on the corner of the box the way the
            // avatar's own curve does.
            div()
                .absolute()
                .bottom(px(-gap))
                .right(px(-gap))
                .size(px(dot + gap * 2.0))
                .rounded_full()
                .bg(palette.surface)
                .flex()
                .items_center()
                .justify_center()
                .child(shape(connection, dot, tint, palette.surface)),
        )
}

/// The badge itself: a disc, or a disc with a bite out of it. The bite is drawn
/// in the colour of the hole around the dot, which is what makes it read as
/// absent rather than as a second colour.
fn shape(connection: petunia_data::Connection, dot: f32, tint: Hsla, behind: Hsla) -> Div {
    let bite = |size: f32, offset: f32| {
        div()
            .absolute()
            .top(px(offset))
            .left(px(offset))
            .size(px(size))
            .rounded_full()
            .bg(behind)
    };

    let disc = div()
        .relative()
        .flex_none()
        .size(px(dot))
        .rounded_full()
        .bg(tint);

    match connection {
        petunia_data::Connection::Connected => disc,
        // A crescent pointing away from the corner, which is Discord's idle.
        petunia_data::Connection::Reconnecting => {
            disc.child(bite(dot * 0.62, -dot * 0.12))
        }
        // A ring, which is Discord's offline.
        petunia_data::Connection::Connecting => disc.child(bite(dot * 0.5, dot * 0.25)),
    }
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
    preview: Option<&'a str>,
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
    const EDGE: f32 = FACE + 8.0;

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
                .child(avatar(line.picture, line.name, line.seed, FACE, palette)),
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
                                        line.preview.unwrap_or_default().to_owned(),
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
