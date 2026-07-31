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

/// What this message is doing besides being sent: answering one, or replacing
/// one. Both are cancelled by Escape.
#[derive(Debug, Clone)]
pub enum Intent {
    Reply { target: MessageId, summary: String },
    Edit { target: MessageId },
}

/// The composer card. A rounded panel floating over the conversation with its
/// controls inside it, a context strip beneath, and whatever the message is
/// carrying stacked above.
pub struct Composer {
    store: Entity<Store>,
    input: Entity<InputState>,
    intent: Option<Intent>,
    attachments: Vec<PathBuf>,
    formatting: bool,
    /// Which pack the sticker picker is showing, or `None` when it is closed.
    stickers: Option<stickers::Showing>,
    /// What is typed into the picker's own filter, which is not the message.
    sticker_query: Entity<InputState>,
    /// When the last typing indicator went out, so the re-send is throttled.
    announced: Option<Instant>,
    _subscriptions: Vec<Subscription>,
}

impl Composer {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
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

        let subscriptions = vec![
            cx.subscribe_in(&input, window, Self::on_input),
            cx.subscribe_in(&sticker_query, window, |this: &mut Self, _, _: &InputEvent, _, cx| {
                cx.notify();
                let _ = this;
            }),
        ];

        Self {
            store,
            input,
            intent: None,
            attachments: Vec::new(),
            formatting: false,
            stickers: None,
            sticker_query,
            announced: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
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
        if self.stickers.take().is_some() {
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
        self.stickers = None;
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
        let (padding_x, title, typing, packs) = {
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
            // Only cloned when the picker is up, because a pack is a list of
            // stickers and this runs every frame.
            let query = self.sticker_query.read(cx).value().to_string();
            let packs = match self.stickers.is_some() {
                true => store
                    .state()
                    .map(|state| state.sticker_packs.clone())
                    .unwrap_or_default(),
                false => Vec::new(),
            };
            (padding_x, title, typing, (packs, query))
        };
        let (packs, query) = packs;

        let field = div()
            .flex()
            .items_end()
            .gap_1p5()
            .px_2()
            .py_1p5()
            .rounded(px(kit::RADIUS_LG))
            .bg(palette.elevated)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    // Sized down for its padding, not its text: the field's own
                    // vertical padding is most of what made the card tall, and
                    // small and medium draw the same size of type.
                    .child(
                        Input::new(&self.input)
                            .appearance(false)
                            .bordered(false)
                            .small(),
                    ),
            )
            .child(
                div()
                    .id("formatting")
                    .flex_none()
                    .size(px(24.0))
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
            .child(
                div()
                    .id("stickers")
                    .flex_none()
                    .size(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .when(self.stickers.is_some(), |this| this.bg(palette.active))
                    .hover(|this| this.bg(palette.hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.stickers = match this.stickers {
                                Some(_) => None,
                                None => Some(stickers::Showing::default()),
                            };
                            cx.notify();
                        }),
                    )
                    // The set ships no sticker glyph, and the smiley this drew
                    // instead read as an emoji picker -- which is a different
                    // control that this one is not.
                    .child(kit::glyph(
                        "icons/sticker.svg",
                        15.0,
                        if self.stickers.is_some() {
                            palette.text_dim
                        } else {
                            palette.text_muted
                        },
                    )),
            )
            .child(kit::icon_button(
                "attach",
                IconName::Plus,
                &palette,
                cx.listener(|this, _, window, cx| this.pick_files(window, cx)),
            ))
            .child(send(
                &palette,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ));

        kit::measured()
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
            .when_some(self.stickers.clone(), |this, showing| {
                this.child(
                    stickers::Picker {
                        packs: &packs,
                        showing: &showing,
                        query: &query,
                        search: &self.sticker_query,
                        theme: &palette,
                        on_pack: std::rc::Rc::new(cx.listener(
                            |this: &mut Self, showing: &stickers::Showing, _, cx| {
                                this.stickers = Some(showing.clone());
                                cx.notify();
                            },
                        )),
                        on_pick: std::rc::Rc::new(cx.listener(
                            |this: &mut Self, chosen: &stickers::Chosen, _, cx| {
                                this.send_sticker(chosen.clone(), cx)
                            },
                        )),
                    }
                    .render(),
                )
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

    if !path.exists()
        && let Err(error) = std::fs::write(&path, bytes.as_ref())
    {
        tracing::warn!(%error, "could not save a pasted image");
        return None;
    }
    Some(path)
}

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

/// Signal's own formatting. Each button is drawn in the style it applies, so it
/// shows what it does rather than needing an icon to say so -- the icon set has
/// no bold or italic, and the box-drawing glyph a spoiler wanted is simply
/// absent from the system font.
fn toolbar(palette: &Theme, cx: &mut Context<Composer>) -> Div {
    const MARKS: [(&str, Style); 5] = [
        ("bold", Style::Bold),
        ("italic", Style::Italic),
        ("strikethrough", Style::Strikethrough),
        ("monospace", Style::Monospace),
        ("spoiler", Style::Spoiler),
    ];

    div()
        .flex()
        .items_center()
        .gap_0p5()
        .p_1()
        .rounded(px(kit::RADIUS))
        .bg(palette.elevated)
        .border_1()
        .border_color(palette.border)
        .children(MARKS.map(|(id, style)| {
            // A fixed square with everything centred inside it, so a glyph and
            // an icon sit on the same baseline instead of drifting apart.
            let button = div()
                .id(id)
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
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Composer, _, window, cx| {
                        this.mark(style, window, cx)
                    }),
                );

            match style {
                Style::Bold => button.font_weight(kit::STRONG).child("B"),
                Style::Italic => button.italic().child("I"),
                Style::Strikethrough => button.line_through().child("S"),
                Style::Monospace => button
                    .font_family(palette.typography.mono.clone())
                    .child("M"),
                _ => button.child(kit::icon(IconName::EyeOff, 15.0, palette.text_dim)),
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
        .size(px(26.0))
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
