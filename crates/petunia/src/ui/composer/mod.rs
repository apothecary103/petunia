pub mod emoji;
pub mod picker;
pub mod stickers;

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    ClipboardEntry, Context, Div, Entity, ImageFormat, MouseButton, SharedString, Subscription,
    Window, div, px,
};
use gpui_component::{IconName, Sizable as _};
use gpui_component::input;
use gpui_component::input::{Input, InputEvent, InputState};

use super::kit;
use crate::actions;
use petunia_config::Theme;
use petunia_data::message::markup;
use petunia_data::message::range::Style;
use petunia_data::{MessageId, Thread};
use petunia_signal::Command;
use crate::store::Store;
use crate::theme::ActivePalette;

/// Signal re-sends "started" about every ten seconds while typing continues, and
/// the receiving side ages an indicator out after fifteen.
const TYPING_INTERVAL: Duration = Duration::from_secs(10);

/// What the card keeps clear at every edge. One number rather than a horizontal
/// and a vertical, because the controls are square and sitting the same distance
/// from the bottom as from the right is what makes the corner look right.
const CARD_PADDING: f32 = 8.0;

/// How big every control inside the card is. `kit::icon_button`'s size, since
/// two of the four are icon buttons and a row of squares that are nearly the
/// same size reads as a mistake rather than as a hierarchy.
const CONTROL: f32 = 26.0;

/// What this message is doing besides being sent: answering one, or replacing
/// one. Both are cancelled by Escape.
#[derive(Debug, Clone)]
pub enum Intent {
    Reply { target: MessageId, summary: String },
    Edit { target: MessageId },
}

/// A poll was asked for. The conversation opens it, since a dialog answers to
/// the workspace rather than to the card that asked for one.
pub struct RequestPoll;

/// The plus was pressed, at this point on screen. What goes in the menu is the
/// composer's business; where a menu is drawn is the workspace's, so this only
/// says where the click was.
pub struct RequestMore(pub gpui::Point<gpui::Pixels>);

/// A sticker in the picker was right-clicked. What the menu offers -- keeping it
/// to hand, opening the pack it is from -- is decided where the store is, so
/// this only says which sticker and where the click was.
pub struct RequestStickerMenu {
    pub chosen: stickers::Chosen,
    pub at: gpui::Point<gpui::Pixels>,
}

impl gpui::EventEmitter<RequestPoll> for Composer {}
impl gpui::EventEmitter<RequestMore> for Composer {}
impl gpui::EventEmitter<RequestStickerMenu> for Composer {}

/// What was typed in a conversation and not sent. Everything the field is
/// carrying, because all of it was meant for the conversation it was written in
/// -- a reply banner and a file picked out are as much part of the message as
/// the words are, and following the reader into the next conversation is how one
/// of them ends up in the wrong window.
#[derive(Debug, Default)]
struct Draft {
    body: String,
    intent: Option<Intent>,
    attachments: Vec<PathBuf>,
}

impl Draft {
    fn is_empty(&self) -> bool {
        self.body.trim().is_empty() && self.intent.is_none() && self.attachments.is_empty()
    }
}

/// The composer card. A rounded panel floating over the conversation with its
/// controls inside it, a context strip beneath, and whatever the message is
/// carrying stacked above.
pub struct Composer {
    store: Entity<Store>,
    input: Entity<InputState>,
    intent: Option<Intent>,
    attachments: Vec<PathBuf>,
    /// Every conversation's unsent message but the one in the field, which is
    /// held by the field itself until the reader leaves it.
    drafts: std::collections::HashMap<Thread, Draft>,
    /// Whose draft the field is holding, so a change of conversation knows where
    /// to put it away.
    holding: Option<Thread>,
    formatting: bool,
    /// Which tab of the picker is showing, or `None` when the panel is closed.
    /// One panel with two tabs rather than two panels: there is only room for
    /// one above the card, and picking a sticker and picking an emoji are the
    /// same gesture.
    picker: Option<picker::Tab>,
    /// Which pack the sticker tab is showing. Kept while the panel is closed, so
    /// reopening it shows what was last being browsed.
    stickers: stickers::Showing,
    /// What is typed into each tab's own filter, which is not the message.
    sticker_query: Entity<InputState>,
    /// Which group the emoji tab is showing.
    emoji: emoji::Showing,
    emoji_query: Entity<InputState>,
    /// When the last typing indicator went out, so the re-send is throttled.
    announced: Option<Instant>,
    _subscriptions: Vec<Subscription>,
}

impl Composer {
    pub fn new(
        store: Entity<Store>,
        kept: Vec<(Thread, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .submit_on_enter(true)
                .auto_grow(1, 8)
                .placeholder("Message")
        });

        let sticker_query = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search stickers")
        });

        let emoji_query = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search emoji")
        });

        let subscriptions = vec![
            cx.subscribe_in(&input, window, Self::on_input),
            cx.subscribe_in(&sticker_query, window, |this: &mut Self, _, _: &InputEvent, _, cx| {
                cx.notify();
                let _ = this;
            }),
            cx.subscribe_in(&emoji_query, window, |this: &mut Self, _, _: &InputEvent, _, cx| {
                cx.notify();
                let _ = this;
            }),
            // The field belongs to whichever conversation is open, so the store
            // saying which one that is now is when the drafts change hands.
            cx.observe_in(&store, window, Self::follow_active),
        ];

        let drafts = kept
            .into_iter()
            .filter(|(_, body)| !body.trim().is_empty())
            .map(|(thread, body)| {
                let draft = Draft {
                    body,
                    ..Draft::default()
                };
                (thread, draft)
            })
            .collect();

        Self {
            store,
            input,
            intent: None,
            attachments: Vec::new(),
            drafts,
            holding: None,
            formatting: false,
            picker: None,
            stickers: stickers::Showing::default(),
            sticker_query,
            emoji: emoji::Showing::default(),
            emoji_query,
            announced: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Puts the open conversation's draft in the field, and whatever was in the
    /// field away under the conversation it was written for.
    fn follow_active(&mut self, _store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.store.read(cx).active().cloned();
        if active == self.holding {
            return;
        }

        if let Some(left) = self.holding.take() {
            let draft = Draft {
                body: self.input.read(cx).value().to_string(),
                intent: self.intent.take(),
                attachments: std::mem::take(&mut self.attachments),
            };
            // Nobody should be watching a typing indicator for a field that is
            // no longer in front of the person who filled it.
            self.stop_typing(&left, cx);
            match draft.is_empty() {
                true => self.drafts.remove(&left),
                false => self.drafts.insert(left, draft),
            };
        }

        let draft = active
            .as_ref()
            .and_then(|thread| self.drafts.remove(thread))
            .unwrap_or_default();
        self.holding = active;
        self.intent = draft.intent;
        self.attachments = draft.attachments;
        // A picker is about the message being written, so it does not follow the
        // reader into the next conversation either.
        self.picker = None;
        self.input
            .update(cx, |input, cx| input.set_value(draft.body, window, cx));
        cx.notify();
    }

    /// Every unsent body, the one in the field included. Kept across launches,
    /// which is why it is the words and nothing else: an attachment is a path,
    /// and the reply it answers may not be loaded next time.
    pub fn drafts(&self, cx: &gpui::App) -> Vec<(Thread, String)> {
        let held = self
            .holding
            .clone()
            .map(|thread| (thread, self.input.read(cx).value().to_string()));

        self.drafts
            .iter()
            .map(|(thread, draft)| (thread.clone(), draft.body.clone()))
            .chain(held)
            .filter(|(_, body)| !body.trim().is_empty())
            .collect()
    }

    pub fn is_empty(&self, cx: &gpui::App) -> bool {
        self.input.read(cx).value().trim().is_empty()
    }

    /// Starts a reply. The quoted text is snapshotted here because the recipient
    /// may not have the original.
    pub fn reply_to(
        &mut self,
        target: MessageId,
        summary: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.intent = Some(Intent::Reply { target, summary });
        self.focus(window, cx);
        cx.notify();
    }

    /// Starts an edit, seeding the field with what is being replaced.
    pub fn edit(
        &mut self,
        target: MessageId,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.intent = Some(Intent::Edit { target });
        self.input
            .update(cx, |input, cx| input.set_value(body, window, cx));
        self.focus(window, cx);
        cx.notify();
    }

    pub fn attach(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.attachments.extend(paths);
        cx.notify();
    }

    /// Takes from the clipboard whatever the field cannot: a screenshot arrives
    /// as bytes and a file copied in Finder as a path, and neither is text. Run
    /// in the capture phase, ahead of the field's own paste, and only claimed
    /// when something was actually attached -- so pasting text still types it.
    fn paste(&mut self, _: &input::Paste, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };

        let attached: Vec<_> = item
            .entries()
            .iter()
            .flat_map(|entry| match entry {
                ClipboardEntry::Image(image) => spill(image).into_iter().collect::<Vec<_>>(),
                ClipboardEntry::ExternalPaths(paths) => paths.paths().to_vec(),
                ClipboardEntry::String(_) => Vec::new(),
            })
            .collect();

        if attached.is_empty() {
            return;
        }
        cx.stop_propagation();
        self.attach(attached, cx);
    }

    /// Escape. Drops the reply or edit first and only then the attachments, so
    /// one press never throws away more than one thing.
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(intent) = self.intent.take() {
            // An edit put its subject in the field, so leaving that behind would
            // look like a new message the user never wrote.
            if matches!(intent, Intent::Edit { .. }) {
                self.input
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
            cx.notify();
            return true;
        }
        if self.picker.take().is_some() {
            cx.notify();
            return true;
        }
        if !self.attachments.is_empty() {
            self.attachments.clear();
            cx.notify();
            return true;
        }
        false
    }

    /// Sends a sticker, which goes on its own rather than with whatever is
    /// typed: Signal has no way to carry both.
    fn send_sticker(&mut self, chosen: stickers::Chosen, cx: &mut Context<Self>) {
        let Some(thread) = self.store.read(cx).active().cloned() else {
            return;
        };
        self.picker = None;
        self.store.update(cx, |store, cx| {
            store.send_sticker(thread, chosen, cx);
        });
        cx.notify();
    }

    fn on_input(
        &mut self,
        _input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // Shift+Enter has already inserted its newline by the time this
            // arrives, so only the bare press sends.
            InputEvent::PressEnter { shift: false, .. } => self.submit(window, cx),
            InputEvent::Change => self.announce_typing(cx),
            _ => {}
        }
    }

    /// The first keystroke says so, and nothing more often than every ten
    /// seconds; emptying the field says it stopped.
    fn announce_typing(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.store.read(cx).active().cloned() else {
            return;
        };

        if self.is_empty(cx) {
            self.stop_typing(&thread, cx);
            return;
        }
        let due = self
            .announced
            .is_none_or(|when| when.elapsed() >= TYPING_INTERVAL);
        if due {
            self.announced = Some(Instant::now());
            self.store.update(cx, |store, _| {
                store.send(Command::Typing {
                    thread,
                    started: true,
                })
            });
        }
    }

    fn stop_typing(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        if self.announced.take().is_none() {
            return;
        }
        let thread = thread.clone();
        self.store.update(cx, |store, _| {
            store.send(Command::Typing {
                thread,
                started: false,
            })
        });
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread) = self.store.read(cx).active().cloned() else {
            return;
        };
        let typed = self.input.read(cx).value().to_string();
        let (body, ranges) = markup::parse(typed.trim());
        let attachments = std::mem::take(&mut self.attachments);

        if body.is_empty() && attachments.is_empty() {
            return;
        }

        let intent = self.intent.take();
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        // The field is empty now, so nobody should still be watching a typing
        // indicator that will only age out fifteen seconds later.
        self.stop_typing(&thread, cx);

        self.store.update(cx, |store, cx| {
            store.compose(thread, body, ranges, attachments, intent, cx)
        });
        cx.notify();
    }

    /// A toolbar button wraps the selection in the marker it stands for, so what
    /// the button does is visible in the field rather than hidden in state the
    /// composer would have to keep in step with every keystroke.
    fn mark(&mut self, style: Style, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            let text = input.value().to_string();
            let (wrapped, selection) = markup::wrap(&text, input.selected_range(), style);
            input.set_value(wrapped, window, cx);
            input.set_selected_range(selection, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Puts an emoji in at the caret, replacing whatever was selected.
    ///
    /// The picker stays open: picking one is very often picking three, and a
    /// panel that closed on the first would have to be reopened for each of them.
    fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            let current = input.value().to_string();
            let selection = input.selected_range();
            let start = selection.start.min(current.len());
            let end = selection.end.clamp(start, current.len());

            let mut with = String::with_capacity(current.len() + text.len());
            with.push_str(&current[..start]);
            with.push_str(text);
            with.push_str(&current[end..]);

            let at = start + text.len();
            input.set_value(with, window, cx);
            input.set_selected_range(at..at, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// The same, for the markers Signal has no style for.
    fn mark_between(
        &mut self,
        opening: &str,
        closing: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input.update(cx, |input, cx| {
            let text = input.value().to_string();
            let (wrapped, selection) =
                markup::between(&text, input.selected_range(), opening, closing);
            input.set_value(wrapped, window, cx);
            input.set_selected_range(selection, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The platform's own dialog rather than a crate's: gpui already owns the
        // event loop a file picker has to run on.
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            this.update(cx, |this, cx| this.attach(paths, cx)).ok();
        })
        .detach();
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        // Read out of the store and let the borrow go: everything below needs
        // the context mutably to build its listeners.
        let (padding_x, title, typing, packs, favourites) = {
            let store = self.store.read(cx);
            // The same padding the message list uses. Both are capped at the
            // reading measure and centred, so a card with a padding of its own
            // sits a few pixels off the column it belongs to -- at any density
            // or scale other than the default, visibly.
            let padding_x = store.config.messages.spacing().padding_x;
            let title = store
                .active()
                .zip(store.state())
                .map(|(thread, state)| state.title(thread))
                .unwrap_or_default();
            let typing = store
                .active()
                .zip(store.state())
                .and_then(|(thread, state)| describe_typing(state, thread));
            // Only cloned when the sticker tab is up, because a pack is a list
            // of stickers and this runs every frame.
            let query = self.sticker_query.read(cx).value().to_string();
            let showing_stickers = self.picker == Some(picker::Tab::Stickers);
            let packs = match showing_stickers {
                true => store.sticker_packs().to_vec(),
                false => Vec::new(),
            };
            let favourites = match showing_stickers {
                true => store.favourites.kept().to_vec(),
                false => Vec::new(),
            };
            let unreadable = store.unreadable_packs();
            (padding_x, title, typing, (packs, query, unreadable), favourites)
        };
        let (packs, query, unreadable_packs) = packs;

        let field = div()
            .flex()
            .items_end()
            .gap_1p5()
            .p(px(CARD_PADDING))
            .rounded(px(kit::RADIUS_LG))
            .bg(palette.elevated)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    // As tall as the controls beside it, so one line of text is
                    // centred on them rather than sitting on the floor of the
                    // card while they sit in the middle of theirs.
                    .min_h(px(CONTROL))
                    // Sized down for its padding, not its text: the field's own
                    // vertical padding is most of what made the card tall, and
                    // small and medium draw the same size of type.
                    //
                    // And then stripped of that padding entirely. The library
                    // insets a small field by eight across and two down, which
                    // is the card's own padding again on one side only -- so the
                    // words started sixteen pixels in while the send button sat
                    // eight from the other edge, and their baselines were two
                    // apart. The card is what holds the padding here.
                    .child(
                        Input::new(&self.input)
                            .appearance(false)
                            .bordered(false)
                            .small()
                            .px_0()
                            .py_0(),
                    ),
            )
            .child(
                div()
                    .id("formatting")
                    .flex_none()
                    .size(px(CONTROL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .when(self.formatting, |this| this.bg(palette.active))
                    .hover(|this| this.bg(palette.hover))
                    .text_size(px(12.0))
                    .text_color(if self.formatting {
                        palette.text_dim
                    } else {
                        palette.text_muted
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.formatting = !this.formatting;
                            cx.notify();
                        }),
                    )
                    .child("Aa"),
            )
            // One control for both pickers. Stickers and emoji are the same
            // gesture -- reach for a picture, put it in the message -- and the
            // panel it opens is what says which of the two you are after.
            .child(
                div()
                    .id("stickers")
                    .flex_none()
                    .size(px(CONTROL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .when(self.picker.is_some(), |this| this.bg(palette.active))
                    .hover(|this| this.bg(palette.hover))
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new("Stickers and emoji")
                            .build(window, cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.picker = match this.picker {
                                Some(_) => None,
                                None => Some(picker::Tab::default()),
                            };
                            cx.notify();
                        }),
                    )
                    // The set ships no sticker glyph, and the smiley this drew
                    // instead read as an emoji picker -- which is one of the two
                    // things behind this now, rather than the whole of it.
                    .child(kit::glyph(
                        "icons/sticker.svg",
                        15.0,
                        match self.picker.is_some() {
                            true => palette.text_dim,
                            false => palette.text_muted,
                        },
                    )),
            )
            // One plus rather than a row of verbs. A poll is a rare thing to send
            // and a permanent button for it sat beside the file picker as though
            // the two were equally often wanted; folded into a menu, the card
            // keeps a control per *kind* of thing instead of one per thing.
            .child(kit::icon_button(
                "more",
                IconName::Plus,
                &palette,
                cx.listener(|_, event: &gpui::MouseDownEvent, _, cx| {
                    cx.emit(RequestMore(event.position))
                }),
            ))
            .child(send(
                &palette,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ));


        kit::column()
            .flex()
            .flex_col()
            .gap_1()
            .px(px(padding_x))
            .pb_2p5()
            .pt_1p5()
            .on_action(
                cx.listener(|this, _: &actions::AttachFile, window, cx| {
                    this.pick_files(window, cx)
                }),
            )
            // Captured rather than bubbled: the field handles Paste and does not
            // pass it on, so a listener behind it would never see an image.
            .capture_action(cx.listener(Self::paste))
            .when_some(typing, |this, who| {
                this.child(
                    div()
                        .px_1()
                        .text_size(px(palette.typography.ui_size - 2.0))
                        .text_color(palette.text_muted)
                        .child(SharedString::from(who)),
                )
            })
            .when_some(self.intent.clone(), |this, intent| {
                this.child(banner(&intent, &palette, cx))
            })
            .when(!self.attachments.is_empty(), |this| {
                this.child(strip(&self.attachments, &palette, cx))
            })
            .when_some(self.picker, |this, tab| {
                let body = match tab {
                    picker::Tab::Stickers => stickers::Picker {
                        packs: &packs,
                        unreadable: unreadable_packs,
                        showing: &self.stickers,
                        query: &query,
                        search: &self.sticker_query,
                        theme: &palette,
                        favourites: &favourites,
                        on_pack: std::rc::Rc::new(cx.listener(
                            |this: &mut Self, showing: &stickers::Showing, _, cx| {
                                this.stickers = showing.clone();
                                cx.notify();
                            },
                        )),
                        on_pick: std::rc::Rc::new(cx.listener(
                            |this: &mut Self, chosen: &stickers::Chosen, _, cx| {
                                this.send_sticker(chosen.clone(), cx)
                            },
                        )),
                        on_menu: std::rc::Rc::new(cx.listener(
                            |_: &mut Self,
                             (chosen, at): &(stickers::Chosen, gpui::Point<gpui::Pixels>),
                             _,
                             cx| {
                                cx.emit(RequestStickerMenu {
                                    chosen: chosen.clone(),
                                    at: *at,
                                })
                            },
                        )),
                    }
                    .render(),
                    picker::Tab::Emoji => {
                        let query = self.emoji_query.read(cx).value().to_string();
                        emoji::Picker {
                            showing: self.emoji,
                            query: &query,
                            search: &self.emoji_query,
                            theme: &palette,
                            on_group: std::rc::Rc::new(cx.listener(
                                |this: &mut Self, showing: &emoji::Showing, _, cx| {
                                    this.emoji = *showing;
                                    cx.notify();
                                },
                            )),
                            on_pick: std::rc::Rc::new(cx.listener(
                                |this: &mut Self, picked: &str, window, cx| {
                                    this.insert(picked, window, cx)
                                },
                            )),
                        }
                        .render()
                    }
                };

                this.child(picker::panel(
                    tab,
                    &palette,
                    std::rc::Rc::new(cx.listener(
                        |this: &mut Self, tab: &picker::Tab, _, cx| {
                            this.picker = Some(*tab);
                            cx.notify();
                        },
                    )),
                    body,
                ))
            })
            .when(self.formatting, |this| this.child(toolbar(&palette, cx)))
            .child(field)
            .child(context(&title, &palette))
    }
}

/// Writes a pasted image where the send path can read it. Everything downstream
/// takes a path, and the content type Signal is told comes from the extension --
/// so the file is named for what it holds, and a format no phone will draw is
/// re-encoded rather than sent as bytes nobody can open.
///
/// The temp directory is right: the file only has to outlive the composer, since
/// the upload adopts a copy into the media cache. The name is the clipboard's own
/// hash of the bytes, so pasting one screenshot twice writes one file.
fn spill(image: &gpui::Image) -> Option<PathBuf> {
    let (extension, bytes) = match image.format {
        ImageFormat::Png => ("png", Cow::Borrowed(image.bytes.as_slice())),
        ImageFormat::Jpeg => ("jpg", Cow::Borrowed(image.bytes.as_slice())),
        ImageFormat::Gif => ("gif", Cow::Borrowed(image.bytes.as_slice())),
        ImageFormat::Webp => ("webp", Cow::Borrowed(image.bytes.as_slice())),
        // A vector has no pixels to re-encode until something picks a size. The
        // markup is on the clipboard as text too, so leaving this alone lets the
        // field paste that instead of attaching a file Signal has no type for.
        ImageFormat::Svg => return None,
        _ => ("png", Cow::Owned(png(&image.bytes)?)),
    };

    let directory = std::env::temp_dir().join("petunia");
    std::fs::create_dir_all(&directory).ok()?;
    let path = directory.join(format!("{:016x}.{extension}", image.id()));

    // Written under a name of its own and moved into place. The name is a hash of
    // the bytes, so two pastes of one image aim at the same path, and a `write`
    // truncates before it fills -- which hands whoever asked second a file that
    // is half an image. A rename is atomic, and the bytes being identical is what
    // makes overwriting the winner harmless.
    if !path.exists() {
        let staging = directory.join(format!(
            "{:016x}.{}.part",
            image.id(),
            SPILLED.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        if let Err(error) = std::fs::write(&staging, bytes.as_ref())
            .and_then(|()| std::fs::rename(&staging, &path))
        {
            tracing::warn!(%error, "could not save a pasted image");
            return None;
        }
    }
    Some(path)
}

/// Tells one in-flight write of the same image from another.
static SPILLED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn png(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoded = image::load_from_memory(bytes)
        .inspect_err(|error| tracing::warn!(%error, "could not read a pasted image"))
        .ok()?;

    let mut encoded = Vec::new();
    decoded
        .write_to(&mut std::io::Cursor::new(&mut encoded), image::ImageFormat::Png)
        .ok()?;
    Some(encoded)
}

fn describe_typing(state: &petunia_data::State, thread: &Thread) -> Option<String> {
    let names: Vec<_> = state
        .typing(thread)
        .into_iter()
        .map(|who| state.name_of(who))
        .collect();

    match names.as_slice() {
        [] => None,
        [one] => Some(format!("{one} is typing…")),
        [rest @ .., last] => Some(format!("{} and {last} are typing…", rest.join(", "))),
    }
}

/// What this message is answering or replacing, with the way out beside it.
fn banner(intent: &Intent, palette: &Theme, cx: &mut Context<Composer>) -> Div {
    let (label, detail) = match intent {
        Intent::Reply { summary, .. } => ("Replying to", summary.clone()),
        Intent::Edit { .. } => ("Editing", String::new()),
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1p5()
        .rounded(px(kit::RADIUS))
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .child(
            div()
                .flex_none()
                .text_size(px(palette.typography.ui_size - 2.0))
                .text_color(palette.text_muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(palette.typography.ui_size - 1.0))
                .text_color(palette.text_dim)
                .child(SharedString::from(detail)),
        )
        .child(kit::icon_button(
            "cancel-intent",
            IconName::Close,
            palette,
            cx.listener(|this: &mut Composer, _, window, cx| {
                this.cancel(window, cx);
            }),
        ))
}

/// What is going out with the message, each with its own way off.
fn strip(paths: &[PathBuf], palette: &Theme, cx: &mut Context<Composer>) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_1p5()
        .children(paths.iter().enumerate().map(|(index, path)| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();

            div()
                .id(SharedString::from(format!("attached-{index}")))
                .flex()
                .items_center()
                .gap_1p5()
                .px_2()
                .py_1()
                .rounded(px(kit::RADIUS))
                .bg(palette.elevated)
                .border_1()
                .border_color(palette.border)
                .child(thumbnail(path, palette))
                .child(
                    div()
                        .max_w(px(140.0))
                        .truncate()
                        .text_size(px(palette.typography.ui_size - 2.0))
                        .text_color(palette.text_dim)
                        .child(SharedString::from(name)),
                )
                .child(kit::icon_button(
                    SharedString::from(format!("drop-{index}")),
                    IconName::Close,
                    palette,
                    cx.listener(move |this: &mut Composer, _, _, cx| {
                        if index < this.attachments.len() {
                            this.attachments.remove(index);
                            cx.notify();
                        }
                    }),
                ))
        }))
}

/// A picture of what is being sent when it is a picture, and the kind of thing
/// it is otherwise.
fn thumbnail(path: &Path, palette: &Theme) -> gpui::AnyElement {
    let kind = petunia_data::attachment::content_type(path);
    if kind.starts_with("image/") {
        return super::image::cropped(path, 28.0)
            .rounded(px(4.0))
            .into_any_element();
    }

    let icon = match kind.split('/').next().unwrap_or_default() {
        "video" => IconName::Play,
        "audio" => IconName::Bell,
        _ => IconName::File,
    };
    kit::icon(icon, 16.0, palette.text_muted).into_any_element()
}

/// What a toolbar button writes around the selection, and how the button is
/// drawn.
///
/// The five Signal styles go through the style, so the marker table in `markup`
/// stays the one place a spelling is decided. The last three are markers Signal
/// has no style for: a fence, whose halves differ, and maths, which travels as
/// what was typed. They are on the same bar because to somebody writing a
/// message they are the same kind of thing.
struct Mark {
    id: &'static str,
    tip: &'static str,
    face: Face,
    opening: &'static str,
    closing: &'static str,
    style: Option<Style>,
}

enum Face {
    Bold,
    Italic,
    Strike,
    Mono(&'static str),
    /// A variable, set the way maths sets one.
    Math(&'static str),
    Glyph(&'static str),
    Spoiler,
}

const MARKS: [Mark; 8] = [
    Mark { id: "bold", tip: "Bold", face: Face::Bold, opening: "", closing: "", style: Some(Style::Bold) },
    Mark { id: "italic", tip: "Italic", face: Face::Italic, opening: "", closing: "", style: Some(Style::Italic) },
    Mark { id: "strikethrough", tip: "Strikethrough", face: Face::Strike, opening: "", closing: "", style: Some(Style::Strikethrough) },
    Mark { id: "code", tip: "Code", face: Face::Mono("‹›"), opening: "", closing: "", style: Some(Style::Monospace) },
    Mark { id: "code-block", tip: "Code block", face: Face::Mono("{ }"), opening: "```\n", closing: "\n```", style: None },
    Mark { id: "maths", tip: "Maths", face: Face::Math("x"), opening: "$", closing: "$", style: None },
    Mark { id: "display-maths", tip: "Maths block", face: Face::Glyph("∑"), opening: "$$\n", closing: "\n$$", style: None },
    Mark { id: "spoiler", tip: "Spoiler", face: Face::Spoiler, opening: "", closing: "", style: Some(Style::Spoiler) },
];

/// Each button is drawn in the style it applies, so it shows what it does rather
/// than needing an icon to say so -- the icon set has no bold or italic, and the
/// box-drawing glyph a spoiler wanted is simply absent from the system font.
/// What is left over is a tooltip, for the three whose glyph is a convention
/// rather than a picture of the result.
fn toolbar(palette: &Theme, cx: &mut Context<Composer>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .p_1()
        .rounded(px(kit::RADIUS))
        .bg(palette.elevated)
        .border_1()
        .border_color(palette.border)
        .children(MARKS.iter().map(|mark| {
            let (opening, closing, style) = (mark.opening, mark.closing, mark.style);
            let tip = mark.tip;

            // A fixed square with everything centred inside it, so a glyph and
            // an icon sit on the same baseline instead of drifting apart.
            let button = div()
                .id(mark.id)
                .size(px(26.0))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .cursor_pointer()
                .hover(|this| this.bg(palette.hover))
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text_dim)
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tip).build(window, cx)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Composer, _, window, cx| match style {
                        Some(style) => this.mark(style, window, cx),
                        None => this.mark_between(opening, closing, window, cx),
                    }),
                );

            match mark.face {
                Face::Bold => button.font_weight(kit::STRONG).child("B"),
                Face::Italic => button.italic().child("I"),
                Face::Strike => button.line_through().child("S"),
                Face::Mono(glyph) => button
                    .font_family(palette.typography.mono.clone())
                    .text_size(px(palette.typography.ui_size - 1.0))
                    .child(glyph),
                Face::Math(glyph) => button.italic().child(glyph),
                Face::Glyph(glyph) => button.child(glyph),
                Face::Spoiler => button.child(kit::icon(IconName::EyeOff, 15.0, palette.text_dim)),
            }
        }))
}

/// The strip under the composer, carrying whatever is true about where this
/// message is going rather than another row of buttons.
fn context(title: &str, palette: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_1()
        .text_size(px(palette.typography.ui_size - 3.0))
        .text_color(palette.text_muted)
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(SharedString::from(if title.is_empty() {
                    "Signal".to_string()
                } else {
                    format!("To {title}")
                })),
        )
        .child(
            div()
                .flex_none()
                .child("Enter to send · Shift+Enter for a new line"),
        )
}

/// The one bright thing on the screen, so the eye knows where the action is.
fn send(
    palette: &Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id("send")
        .flex_none()
        .size(px(CONTROL))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .bg(palette.accent)
        .text_size(px(13.0))
        .text_color(palette.on_accent)
        .on_mouse_down(MouseButton::Left, on_click)
        .child("↑")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(format: image::ImageFormat) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .unwrap();
        bytes
    }

    /// Signal is told what an attachment is from its extension, so the name has
    /// to say what the bytes actually are.
    #[test]
    fn a_pasted_png_is_written_as_a_png() {
        let image = gpui::Image::from_bytes(ImageFormat::Png, encoded(image::ImageFormat::Png));

        let path = spill(&image).expect("written");

        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(
            petunia_data::attachment::content_type(&path),
            "image/png"
        );
        assert_eq!(std::fs::read(&path).unwrap(), image.bytes);
    }

    /// A screenshot pasted twice is one file, because the name is the clipboard's
    /// own hash of the bytes.
    #[test]
    fn the_same_image_lands_at_the_same_path() {
        let bytes = encoded(image::ImageFormat::Png);
        let once = gpui::Image::from_bytes(ImageFormat::Png, bytes.clone());
        let twice = gpui::Image::from_bytes(ImageFormat::Png, bytes);

        assert_eq!(spill(&once), spill(&twice));
    }

    /// A format no phone will draw is re-encoded rather than sent as bytes
    /// nobody can open.
    #[test]
    fn an_exotic_format_is_re_encoded_as_png() {
        let image = gpui::Image::from_bytes(ImageFormat::Bmp, encoded(image::ImageFormat::Bmp));

        let path = spill(&image).expect("written");

        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(
            image::guess_format(&std::fs::read(&path).unwrap()).unwrap(),
            image::ImageFormat::Png
        );
    }

    /// A vector has no pixels until something picks a size, and the markup is on
    /// the clipboard as text anyway -- so the field pastes that instead.
    #[test]
    fn an_svg_is_left_to_the_text_field() {
        let image = gpui::Image::from_bytes(ImageFormat::Svg, b"<svg/>".to_vec());

        assert!(spill(&image).is_none());
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused() {
        let image = gpui::Image::from_bytes(ImageFormat::Tiff, b"not an image".to_vec());

        assert!(spill(&image).is_none());
    }
}
