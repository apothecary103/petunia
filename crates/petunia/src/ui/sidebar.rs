use std::path::Path;

use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, Hsla, MouseButton, SharedString, Stateful, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{IconName, Sizable};

use super::avatar::avatar;
use super::kit;
use super::relative;
use petunia_config::Theme;
use petunia_data::{Section, Thread, hex};
use petunia_signal::db::search::Hit;
use crate::store::Store;
use crate::theme::ActivePalette;

/// How large a face is in the list. One value for the rows and the identity, so
/// the column has one rhythm rather than two.
///
/// Signal's own size, near enough: the picture is the thing you actually aim at
/// in a list of conversations, and at thirty-four it was smaller than the two
/// lines of text beside it are tall — which made the text the row and the face an
/// ornament on it.
const FACE: f32 = 46.0;

/// The face on the identity row, which is smaller than the ones in the list.
///
/// A row's picture is the thing you aim at; this one is a label on a line that
/// says who you are signed in as, and at the list's size it was the largest
/// object in the column — the account shouting over every conversation in it.
const OWN_FACE: f32 = 34.0;

/// The conversation list: quiet section headers, two-line entries carrying the
/// name, time and preview, and the account's identity pinned to the bottom.
pub struct Sidebar {
    store: Entity<Store>,
    /// Collapsed to avatars. The workspace owns the width and says so; the list
    /// only decides what fits.
    rail: bool,
    /// Narrows the list to the conversations whose name matches, and asks the
    /// store for the messages that do.
    filter: Entity<InputState>,
    /// What was found in the messages themselves, under the conversations that
    /// matched by name. The same query the `cmd-f` sheet asks, because there is
    /// one search in the application and a box that only looked at names would be
    /// a box that quietly disagrees with the other one.
    hits: Vec<Hit>,
    /// Which query `hits` answers. A slower search finishing after a faster one
    /// would otherwise replace newer results with older.
    answered: String,
    _subscriptions: Vec<Subscription>,
}

/// A message under the search box was picked. The workspace opens the
/// conversation and reveals the line, exactly as it does for the search sheet —
/// which is why this is that event rather than one of its own: one kind of
/// result, one way of answering it.
pub use super::search::Chosen;

impl gpui::EventEmitter<Chosen> for Sidebar {}

impl Sidebar {
    pub fn new(
        store: Entity<Store>,
        rail: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &filter,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => this.ask(cx),
                    // The one on top is the one you meant: typing a name and
                    // pressing return is how every list like this is used.
                    InputEvent::PressEnter { .. } => this.open_first(window, cx),
                    _ => {}
                },
            ),
            cx.subscribe(&store, |this, _, event: &crate::store::StoreEvent, cx| {
                if let crate::store::StoreEvent::Found { query, hits } = event {
                    this.receive(query, hits, cx);
                }
            }),
        ];

        Self {
            store,
            rail,
            filter,
            hits: Vec::new(),
            answered: String::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Asks the store what was said, as well as narrowing the names here. The
    /// worker answers on its own thread, so the list keeps whatever it had until
    /// it does rather than emptying while somebody types.
    fn ask(&mut self, cx: &mut Context<Self>) {
        let query = self.filter.read(cx).value().to_string();

        if query.trim().is_empty() {
            self.hits.clear();
            self.answered.clear();
            cx.notify();
            return;
        }
        self.store.update(cx, |store, _| {
            store.send(petunia_signal::Command::Search {
                query,
                within: None,
            })
        });
        cx.notify();
    }

    fn receive(&mut self, query: &str, hits: &[Hit], cx: &mut Context<Self>) {
        if self.filter.read(cx).value().trim() != query.trim() {
            return;
        }
        self.answered = query.to_owned();
        self.hits = hits.to_vec();
        cx.notify();
    }

    pub fn collapse(&mut self, rail: bool, cx: &mut Context<Self>) {
        self.rail = rail;
        cx.notify();
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter
            .update(cx, |filter, cx| filter.set_value("", window, cx));
        cx.notify();
    }

    /// What is typed, folded for comparison. Empty when there is nothing to
    /// match against, which is the same answer as "everything matches" — and on
    /// the rail there is no field to have typed it in, so what was left in one
    /// before it collapsed narrows nothing.
    fn wanted(&self, cx: &Context<Self>) -> String {
        match self.rail {
            true => String::new(),
            false => self.filter.read(cx).value().trim().to_lowercase(),
        }
    }

    /// Return takes the top of the list, whichever half it is in: a conversation
    /// if a name matched, and otherwise the newest message that did.
    fn open_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = self.wanted(cx);
        let named = self.store.read(cx).state().and_then(|state| {
            state
                .index
                .grouped()
                .into_iter()
                .flat_map(|(_, entries)| entries)
                .find(|entry| matches(&entry.name, &wanted))
                .map(|entry| entry.thread.clone())
        });

        match named {
            Some(thread) => self.open(thread, cx),
            None => match self.hits.first().cloned() {
                Some(hit) => cx.emit(Chosen(hit)),
                None => return,
            },
        }
        self.clear(window, cx);
    }

    fn reveal(&mut self, hit: Hit, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Chosen(hit));
        self.clear(window, cx);
    }

    fn open(&mut self, thread: Thread, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.activate(thread, cx));
    }
}

/// Whether a conversation answers what was typed. Case-folded substring, which
/// is what everybody means by typing three letters of somebody's name.
fn matches(name: &str, wanted: &str) -> bool {
    wanted.is_empty() || name.to_lowercase().contains(wanted)
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
        let wanted = self.wanted(cx);
        let mut named = 0;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let mut list = div()
            .flex()
            .flex_col()
            .when(rail, |this| this.items_center().gap_2().px_1().pb_2())
            .when(!rail, |this| {
                this.gap_4().px(px(kit::LIST_PADDING)).pb_3()
            });

        for (at, (section, entries)) in state
            .index
            .grouped()
            .into_iter()
            .map(|(section, entries)| {
                let entries: Vec<_> = entries
                    .into_iter()
                    .filter(|entry| matches(&entry.name, &wanted))
                    .collect();
                (section, entries)
            })
            // A heading with nothing under it is a section reporting that the
            // filter matched something it did not.
            .filter(|(_, entries)| !entries.is_empty())
            .enumerate()
        {
            named += entries.len();
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

                let for_menu = thread.clone();

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
                    // Only where the line itself is drawn: the ticks annotate the
                    // preview, and on the rail there is none to annotate.
                    status: entry
                        .preview
                        .as_ref()
                        .filter(|_| show_preview && !rail)
                        .and_then(|preview| preview.status),
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
                        let thread = for_menu.clone();
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
                        this.gap_1().child(kit::heading(icon, label, &palette))
                    })
                    .children(rows),
            );
        }

        // What was *said*, under what was named. Only while there is a query: the
        // messages are the half of the answer that has to be asked for, and a
        // heading with nothing under it would be a section that exists to be
        // empty.
        if !rail && !wanted.is_empty() && !self.hits.is_empty() {
            let hits = self.hits.clone();
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(kit::heading(None, "Messages", &palette))
                    .children(hits.into_iter().map(|hit| {
                        let name = state.title(&hit.thread);
                        let who = state.sender_name(hit.sender);
                        let picture = state.avatar(&hit.thread);
                        let chosen = hit.clone();

                        found(&palette, &hit, &name, &who, picture, &self.answered)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.reveal(chosen.clone(), window, cx)
                                }),
                            )
                    })),
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
                    .child(kit::glyph_button(
                        "new-chat",
                        "icons/compose.svg",
                        &palette,
                        cx.listener(|_, _, window, cx| {
                            window.dispatch_action(Box::new(crate::actions::NewChat), cx)
                        }),
                    )),
            )
            // Above the list rather than over it: the list scrolls, and a field
            // that scrolled away with it would be a field you have to go back
            // for. Not on the rail, where a line of text has nowhere to go.
            .when(!rail, |this| {
                this.child(field(&self.filter, &wanted, &palette, cx.listener(
                    |this, _, window, cx| this.clear(window, cx),
                )))
            })
            .child(
                div()
                    .id("conversations")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(match (rail, state.index.is_empty(), named) {
                        (false, true, _) => {
                            note("Waiting for Signal to sync your conversations…", &palette)
                        }
                        (false, false, 0) if !wanted.is_empty() => {
                            note("No conversation by that name.", &palette)
                        }
                        _ => list.into_any_element(),
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

/// How tall the box is. Fixed rather than arrived at from what is inside it:
/// the clear button is a twenty-eight pixel hit target and the search glyph is
/// fourteen, so a field sized to its contents grew by half its own height the
/// moment the first character was typed and shrank again on the last backspace
/// — the whole list under it stepping down and back with every empty query.
/// Tall enough for the button, so nothing about it depends on whether the
/// button is drawn.
const FIELD: f32 = 32.0;

/// The box that narrows the list. `kit::field` is the box, so the one control in
/// the column that is typed into looks like every other one in the application,
/// and the input inside it brings none of its own padding — a field with two
/// paddings starts its text further in on one side than the icon beside it.
fn field(
    filter: &Entity<InputState>,
    wanted: &str,
    palette: &Theme,
    on_clear: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> Div {
    div()
        .flex_none()
        .px(px(kit::LIST_PADDING))
        .pb_2()
        .child(
            kit::field(palette)
                .h(px(FIELD))
                .py_0()
                .gap_2()
                .child(kit::glyph("icons/search.svg", 14.0, palette.text_muted))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(filter)
                            .small()
                            .appearance(false)
                            .bordered(false),
                    ),
                )
                // Only once there is something to clear: a control that does
                // nothing is a control that has to be tried to find that out.
                .when(!wanted.is_empty(), |this| {
                    this.child(kit::glyph_button(
                        "clear-filter",
                        "icons/close.svg",
                        palette,
                        on_clear,
                    ))
                }),
        )
}

/// One message the search found: who said it, in which conversation, and the
/// line it matched on with the words picked out.
///
/// Narrower than the sheet's version of the same row, because the column is:
/// one line of attribution over one line of the message, and the clock folded
/// into the first rather than given a column of its own.
fn found(
    palette: &Theme,
    hit: &Hit,
    thread: &str,
    sender: &str,
    picture: Option<&Path>,
    query: &str,
) -> Stateful<Div> {
    kit::row(
        SharedString::from(format!("hit-{}-{}", hit.timestamp, hex(hit.thread.seed()))),
        false,
        palette,
    )
    .child(div().pt_0p5().child(avatar(
        picture,
        thread,
        hit.thread.seed(),
        26.0,
        palette,
    )))
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
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(palette.typography.ui_size - 1.0))
                            .font_weight(kit::EMPHASIS)
                            .text_color(palette.text_dim)
                            .child(SharedString::from(match thread == sender {
                                true => thread.to_owned(),
                                false => format!("{thread} · {sender}"),
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(palette.typography.ui_size - 3.0))
                            .text_color(palette.text_muted)
                            .child(SharedString::from(relative::short(hit.timestamp))),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text)
                    .child(super::search::matched(
                        &hit.body.replace('\n', " "),
                        query,
                        palette,
                    )),
            ),
    )
}

fn note(text: &'static str, palette: &Theme) -> gpui::AnyElement {
    div()
        .px(px(kit::INSET))
        .py_6()
        .text_size(px(palette.typography.ui_size - 1.0))
        .text_color(palette.text_muted)
        .child(text)
        .into_any_element()
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
            OWN_FACE,
            palette,
        ),
        OWN_FACE,
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
        .py(px(14.0))
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
        .child(kit::glyph_button(
            "settings",
            "icons/settings.svg",
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
    /// Discord's proportions, taken in a shade smaller. Ten on thirty-two is a
    /// badge for a picture that is a *control*; this one annotates a name, and at
    /// that size it was the loudest thing on the line it is a footnote to.
    const DOT: f32 = 8.0 / 34.0;
    const GAP: f32 = 2.0 / 34.0;

    let tint = match connection {
        petunia_data::Connection::Connected => palette.success,
        petunia_data::Connection::Reconnecting => palette.warning,
        petunia_data::Connection::Connecting => palette.text_muted,
    };
    let dot = (size * DOT).round().max(7.0);
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
    /// How far the last message got, when it was ours. `None` for everybody
    /// else's, which is what a status being absent means.
    status: Option<petunia_data::Status>,
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
    // A name is the row's headline and is set as one -- Semibold, which is what
    // AppKit gives the title line of a two-line list row and what Mail and
    // Messages both use. What unread changes is the colour, not the weight: at
    // one weight for read and another for unread the column has two typefaces in
    // it and every arriving message restyles a line of it.
    let title_color = if line.muted {
        palette.text_muted
    } else if unread || line.selected {
        palette.text
    } else {
        palette.text_dim
    };

    kit::row("row", line.selected, palette)
        .child(div().child(avatar(line.picture, line.name, line.seed, FACE, palette)))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                // The face is taller than the two lines beside it, so they are
                // centred against it rather than hung from its top edge.
                .justify_center()
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
                                .font_weight(kit::EMPHASIS)
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
                                    // The row you are *in* is filled, and the
                                    // quietest grey in the theme was chosen to
                                    // sit on the list's own background rather
                                    // than on that fill -- on it the preview was
                                    // a line you could see was there and not
                                    // read. So the selected row's second line
                                    // steps up, exactly as an unread one does.
                                    .text_color(if unread || line.selected {
                                        palette.text_dim
                                    } else {
                                        palette.text_muted
                                    })
                                    .child(SharedString::from(
                                        line.preview.unwrap_or_default().to_owned(),
                                    )),
                            )
                        })
                        // Signal draws the ticks here as well as in the
                        // conversation, and it is the same question in both
                        // places: has the last thing I said got there. Only ours
                        // carries a status at all, so there is nothing to draw
                        // where the last word was somebody else's.
                        .when_some(line.status, |this, status| {
                            this.child(receipt(status, palette))
                        })
                        .when(unread, |this| {
                            this.child(badge(line.unread, line.mentions, line.muted, palette))
                        }),
                ),
        )
}

/// How far the last thing we said got, on the row that says what it was. The
/// same mark the message itself carries, at the size a line wants.
fn receipt(status: petunia_data::Status, palette: &Theme) -> Div {
    use petunia_data::Status;

    /// Smaller than the mark on the message itself: this one annotates a line
    /// rather than closing one.
    const TICK: f32 = 12.0;

    match status {
        Status::Sending => kit::receipt(kit::Mark::Sent, TICK, palette.border_focus),
        Status::Failed => div()
            .flex_none()
            .child(kit::icon(IconName::TriangleAlert, TICK, palette.danger)),
        Status::Sent => kit::receipt(kit::Mark::Sent, TICK, palette.text_muted),
        Status::Delivered => kit::receipt(kit::Mark::Delivered, TICK, palette.text_muted),
        Status::Read | Status::Viewed => kit::receipt(kit::Mark::Read, TICK, palette.text_muted),
    }
}

/// The count, in the colour of something that wants answering. A muted
/// conversation still counts, but in grey: Signal's own behaviour, and the
/// difference between "there is something here" and "look at this".
///
/// Always the number, and never a dot for the first one. The dot was the quieter
/// shape and it said less than it cost -- a row already says a conversation has
/// something new by putting its name in the emphasis weight, so the badge's whole
/// job is the count.
fn badge(unread: u32, mentions: u32, muted: bool, palette: &Theme) -> Div {
    let fill = match muted {
        true => palette.text_muted,
        false => palette.accent,
    };
    let label = match (unread, mentions > 0 && !muted) {
        // A mention is not a count: it is the reason the count matters, and the
        // one thing about a badge worth another glyph.
        (_, true) => format!("@ {unread}"),
        (many, _) => many.to_string(),
    };

    kit::count(label, fill, palette.on_accent, palette)
}
