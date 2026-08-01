//! Starting a conversation that does not exist yet: type a few letters, pick
//! who.
//!
//! The forward picker's shape, because the question is nearly the same one and
//! answering it a second, different way would be a second thing to learn. What
//! it adds is the two things the switcher cannot do: find somebody who is not in
//! the contact list at all, and pick more than one of them.
//!
//! Somebody found by username or phone number is somebody Signal's contact sync
//! has never mentioned, so there is a round trip behind it -- `Command::LookUp`,
//! answered by `StoreEvent::LookedUp`. Only a query shaped like one of the two
//! is sent: a name and a number is a username, a leading `+` is a phone number,
//! and everything else is only ever matched against the contacts already here.

use std::collections::BTreeSet;

use gpui::prelude::*;
use gpui::{
    Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, SharedString, Window, div,
    px,
};
use uuid::Uuid;

use super::avatar::avatar;
use super::kit;
use crate::store::{Store, StoreEvent};
use crate::theme::ActivePalette;
use petunia_data::{ContactId, Thread};

pub struct Dismissed;

/// Who to talk to. The workspace opens it, because which pane shows a
/// conversation is not this picker's business.
#[derive(Debug, Clone)]
pub struct Started(pub Thread);

/// Who to put in a group. The name is asked for afterwards, by the prompt that
/// already asks for every other single line of text in the application.
#[derive(Debug, Clone)]
pub struct Grouping(pub Vec<Uuid>);

impl gpui::EventEmitter<Dismissed> for NewChat {}
impl gpui::EventEmitter<Started> for NewChat {}
impl gpui::EventEmitter<Grouping> for NewChat {}

/// What the picker is being used for. A group keeps its members as it goes,
/// which is the whole difference: one answer or several.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Chat,
    Group(BTreeSet<Uuid>),
}

/// How a lookup is going, for the one row that reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Lookup {
    /// Nothing asked for yet, or the query has moved on since.
    Idle,
    Looking(String),
    /// A query that was asked and came back with nobody.
    Missing(String),
}

pub struct NewChat {
    store: Entity<Store>,
    focus: FocusHandle,
    query: String,
    mode: Mode,
    selected: usize,
    lookup: Lookup,
    scroll: gpui::ScrollHandle,
}

impl NewChat {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&store, |this: &mut Self, _, event: &StoreEvent, cx| {
            let StoreEvent::LookedUp { query, found } = event else {
                return;
            };
            // An answer to a query the field has moved on from is an answer to
            // nothing, and must not report a stranger as missing.
            if this.query.trim() != query {
                return;
            }
            match found {
                Some(contact) => this.start(contact.uuid, cx),
                None => {
                    this.lookup = Lookup::Missing(query.clone());
                    cx.notify();
                }
            }
        })
        .detach();

        Self {
            store,
            focus: cx.focus_handle(),
            query: String::new(),
            mode: Mode::Chat,
            selected: 0,
            lookup: Lookup::Idle,
            scroll: gpui::ScrollHandle::new(),
        }
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The contacts the query matches, by name, with this account's own row kept
    /// out: Note to Self is a conversation you already have.
    fn hits(&self, cx: &Context<Self>) -> Vec<(Uuid, String)> {
        let query = self.query.trim().to_lowercase();
        self.store
            .read(cx)
            .state()
            .map(|state| {
                let mut hits: Vec<_> = state
                    .contacts
                    .iter()
                    .filter(|contact| contact.uuid != state.aci)
                    .map(|contact| (contact.uuid, state.name_of(contact.uuid)))
                    .filter(|(_, name)| {
                        query.is_empty() || name.to_lowercase().contains(&query)
                    })
                    .collect();
                hits.sort_by_key(|(_, name)| name.to_lowercase());
                hits
            })
            .unwrap_or_default()
    }

    /// Whether what has been typed could name somebody who is not here yet.
    ///
    /// A phone number announces itself with a `+`. A username is a nickname and a
    /// discriminator joined by a dot, which is the one shape that cannot be
    /// confused with a name somebody typed into their phone.
    fn is_address(query: &str) -> bool {
        let query = query.trim();
        if let Some(digits) = query.strip_prefix('+') {
            return digits.len() > 5 && digits.chars().all(char::is_numeric);
        }
        match query.split_once('.') {
            Some((nickname, discriminator)) => {
                !nickname.is_empty()
                    && !discriminator.is_empty()
                    && discriminator.chars().all(char::is_numeric)
            }
            None => false,
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.hits(cx);
        let keystroke = &event.keystroke;

        match keystroke.key.as_str() {
            "escape" => cx.emit(Dismissed),
            "enter" => self.choose(&hits, cx),
            "tab" => self.switch_mode(cx),
            "down" => self.move_by(1, hits.len(), cx),
            "up" => self.move_by(-1, hits.len(), cx),
            "backspace" => {
                self.query.pop();
                self.retyped(cx);
            }
            _ => {
                if let Some(typed) = keystroke.key_char.as_ref().filter(|typed| {
                    !typed.is_empty()
                        && !keystroke.modifiers.platform
                        && !keystroke.modifiers.control
                }) {
                    self.query.push_str(typed);
                    self.retyped(cx);
                }
            }
        }
    }

    /// A lookup answers the query it was asked for and no other, so editing the
    /// field puts the report away rather than leaving it under a different word.
    fn retyped(&mut self, cx: &mut Context<Self>) {
        self.selected = 0;
        self.lookup = Lookup::Idle;
        cx.notify();
    }

    /// Enter: the row you are on, or -- with nothing matching and something
    /// address-shaped typed -- the lookup, so the common case needs no reaching
    /// for the mouse.
    fn choose(&mut self, hits: &[(Uuid, String)], cx: &mut Context<Self>) {
        match hits.get(self.selected) {
            Some(&(uuid, _)) => self.pick(uuid, cx),
            None if Self::is_address(&self.query) => self.look_up(cx),
            None => {}
        }
    }

    fn switch_mode(&mut self, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            Mode::Chat => Mode::Group(BTreeSet::new()),
            Mode::Group(_) => Mode::Chat,
        };
        self.selected = 0;
        cx.notify();
    }

    fn move_by(&mut self, delta: isize, len: usize, cx: &mut Context<Self>) {
        self.selected = super::switcher::step(self.selected, delta, len);
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    /// One person: opens the conversation. In group mode: adds or removes them,
    /// because a picker you cannot change your mind in is a picker that has to be
    /// closed and reopened.
    fn pick(&mut self, uuid: Uuid, cx: &mut Context<Self>) {
        match &mut self.mode {
            Mode::Chat => self.start(uuid, cx),
            Mode::Group(members) => {
                if !members.remove(&uuid) {
                    members.insert(uuid);
                }
                cx.notify();
            }
        }
    }

    fn start(&mut self, uuid: Uuid, cx: &mut Context<Self>) {
        cx.emit(Started(Thread::Contact(ContactId::Aci(uuid))));
        cx.emit(Dismissed);
    }

    fn look_up(&mut self, cx: &mut Context<Self>) {
        let query = self.query.trim().to_owned();
        self.lookup = Lookup::Looking(query.clone());
        self.store.update(cx, |store, _| store.look_up(query));
        cx.notify();
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let Mode::Group(members) = &self.mode else {
            return;
        };
        if members.is_empty() {
            return;
        }
        cx.emit(Grouping(members.iter().copied().collect()));
        cx.emit(Dismissed);
    }
}

impl Focusable for NewChat {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NewChat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let hits = self.hits(cx);
        let pictures: Vec<_> = self
            .store
            .read(cx)
            .state()
            .map(|state| {
                hits.iter()
                    .map(|(uuid, _)| {
                        state
                            .avatar_for(*uuid)
                            .map(|path| path.to_path_buf())
                    })
                    .collect()
            })
            .unwrap_or_default();

        let picked = match &self.mode {
            Mode::Chat => BTreeSet::new(),
            Mode::Group(members) => members.clone(),
        };
        let grouping = matches!(self.mode, Mode::Group(_));

        // Built before the rows: the rows' click handlers borrow `cx`, and these
        // three need it mutably to make handlers of their own.
        let heading = self.heading(&palette, cx).into_any_element();
        let lookup = self.lookup_row(hits.is_empty(), &palette, cx);
        let footer = self
            .footer(&picked, &palette, cx)
            .map(IntoElement::into_any_element);

        let rows = hits.iter().enumerate().map(|(at, (uuid, name))| {
            let picture = pictures.get(at).cloned().flatten();
            let uuid = *uuid;
            let thread = Thread::Contact(ContactId::Aci(uuid));

            kit::row(
                SharedString::from(format!("who-{at}")),
                at == self.selected,
                &palette,
            )
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.pick(uuid, cx)),
            )
            .child(avatar(
                picture.as_deref(),
                name,
                thread.seed(),
                24.0,
                &palette,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text)
                    .child(SharedString::from(name.clone())),
            )
            // Only in group mode, and only for the ones in it: a tick beside
            // everybody would be a row of controls that mostly do nothing.
            .children(
                (grouping && picked.contains(&uuid)).then(|| {
                    kit::icon(gpui_component::IconName::Check, 14.0, palette.accent)
                }),
            )
        });

        kit::scrim(&palette)
            .id("new-chat-scrim")
            .flex_col()
            .items_center()
            .justify_start()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_key))
                    .mt(px(96.0))
                    .w(px(520.0))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .rounded(px(kit::RADIUS_LG))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(heading)
                    .child(
                        div()
                            .id("candidates")
                            .max_h(px(420.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .p_1p5()
                            .children(rows)
                            .children(lookup),
                    )
                    .children(footer),
            )
    }
}

impl NewChat {
    /// What is being started, and what has been typed to find who with.
    fn heading(&self, palette: &petunia_config::Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.query.is_empty();
        let grouping = matches!(self.mode, Mode::Group(_));

        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3p5()
            .py_3()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(kit::icon(
                        gpui_component::IconName::Search,
                        14.0,
                        palette.text_muted,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(palette.typography.message_size))
                            .text_color(match empty {
                                true => palette.text_muted,
                                false => palette.text,
                            })
                            .child(SharedString::from(match empty {
                                true => "Name, username or phone number…".to_owned(),
                                false => self.query.clone(),
                            })),
                    )
                    .child(kit::button(
                        "toggle-group",
                        match grouping {
                            true => "New chat",
                            false => "New group",
                        },
                        kit::Intent::Quiet,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| this.switch_mode(cx)),
                    )),
            )
            .child(
                div()
                    .text_size(px(palette.typography.ui_size - 1.0))
                    .text_color(palette.text_dim)
                    .child(match grouping {
                        true => "Pick everybody who is to be in it.",
                        false => "Search by name, username, or phone number.",
                    }),
            )
    }

    /// The row that finds somebody who is not here, and reports how it went.
    ///
    /// Drawn only when the contacts turned up nothing: while a name still
    /// matches, what was typed is a search rather than an address.
    fn lookup_row(
        &self,
        nothing_matched: bool,
        palette: &petunia_config::Theme,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !nothing_matched {
            return None;
        }
        let note = |text: &str| {
            div()
                .px_2()
                .py_3()
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text_muted)
                .child(SharedString::from(text.to_owned()))
                .into_any_element()
        };

        match &self.lookup {
            Lookup::Looking(query) => Some(note(&format!("Looking for {query}…"))),
            Lookup::Missing(query) => Some(note(&format!("Nobody on Signal is {query}."))),
            Lookup::Idle if Self::is_address(&self.query) => Some(
                kit::row("look-up", true, palette)
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this: &mut Self, _, _, cx| this.look_up(cx)),
                    )
                    .child(kit::icon(
                        gpui_component::IconName::Search,
                        14.0,
                        palette.accent,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(palette.typography.ui_size))
                            .text_color(palette.text)
                            .child(SharedString::from(format!(
                                "Find {} on Signal",
                                self.query.trim()
                            ))),
                    )
                    .into_any_element(),
            ),
            // Nothing typed and nothing to show is an empty contact list, not a
            // search that failed.
            Lookup::Idle if self.query.trim().is_empty() => {
                Some(note("No contacts synced from your phone yet."))
            }
            Lookup::Idle => Some(note(
                "Nobody here by that name. A username needs its number too.",
            )),
        }
    }

    /// The group's own strip: how many are in it, and the button that makes it.
    /// Absent in chat mode, where picking somebody is the whole of the answer.
    fn footer(
        &self,
        picked: &BTreeSet<Uuid>,
        palette: &petunia_config::Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !matches!(self.mode, Mode::Group(_)) {
            return None;
        }

        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .px_3p5()
                .py_3()
                .border_t_1()
                .border_color(palette.border)
                .child(
                    div()
                        .text_size(px(palette.typography.ui_size))
                        .text_color(palette.text_dim)
                        .child(SharedString::from(match picked.len() {
                            0 => "Nobody picked yet".to_owned(),
                            1 => "1 person".to_owned(),
                            many => format!("{many} people"),
                        })),
                )
                // Drawn only once it would do something: a button that refuses
                // is a control that lies about being one.
                .children((!picked.is_empty()).then(|| {
                    kit::button(
                        "create-group",
                        "Continue…",
                        kit::Intent::Primary,
                        palette,
                        cx.listener(|this: &mut Self, _, _, cx| this.create(cx)),
                    )
                })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::NewChat;

    #[test]
    fn a_username_is_a_name_and_a_number() {
        assert!(NewChat::is_address("wren.01"));
        assert!(NewChat::is_address("wren.4823"));
    }

    #[test]
    fn a_phone_number_announces_itself_with_a_plus() {
        assert!(NewChat::is_address("+447700900123"));
        assert!(!NewChat::is_address("+44770"));
        assert!(!NewChat::is_address("+44 7700 900123"));
    }

    /// Everything else is a search over the contacts already here, and must not
    /// send a round trip that can only come back with nobody.
    #[test]
    fn a_name_is_not_an_address() {
        assert!(!NewChat::is_address("wren"));
        assert!(!NewChat::is_address(""));
        assert!(!NewChat::is_address("wren."));
        assert!(!NewChat::is_address(".01"));
        assert!(!NewChat::is_address("wren.tanaka"));
    }
}
