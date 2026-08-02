//! What the wire actually said about one message.
//!
//! Everything else in the window is an interpretation: a summary, a bubble, a
//! tick. This is the message as it arrived, for the times when the answer to
//! "why does it look like that" is in a field nothing draws.

use gpui::prelude::*;
use gpui::{Context, MouseButton, SharedString, Window, div, px};

use super::kit;
use super::relative;
use petunia_data::Message;
use petunia_data::attachment::Blob;
use crate::theme::ActivePalette;

pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for Raw {}

pub struct Raw {
    /// The fields worth naming, resolved when the sheet opens rather than per
    /// frame.
    fields: Vec<(String, String)>,
    /// The whole structure, for everything the list above does not name.
    dump: String,
    focus: gpui::FocusHandle,
}

impl Raw {
    pub fn new(message: &Message, sender: &str, cx: &mut Context<Self>) -> Self {
        Self {
            fields: fields(message, sender),
            dump: format!("{message:#?}"),
            focus: cx.focus_handle(),
        }
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The whole thing, because what this is for is pasting it somewhere.
    fn copy(&self, cx: &mut Context<Self>) {
        let described = self
            .fields
            .iter()
            .map(|(name, value)| format!("{name}: {value}\n"))
            .collect::<String>();

        cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!(
            "{described}\n{}",
            self.dump
        )));
    }
}

/// What a person actually wants to know, in the order they want it: when, who,
/// what state it is in, and what it is carrying.
fn fields(message: &Message, sender: &str) -> Vec<(String, String)> {
    use petunia_data::message::Content;

    let mut fields = vec![
        (
            "Sent".to_owned(),
            match relative::local(message.timestamp()) {
                Some(at) => format!("{}", at.format("%Y-%m-%d %H:%M:%S%.3f %Z")),
                None => "unknown".to_owned(),
            },
        ),
        ("Timestamp".to_owned(), message.timestamp().to_string()),
        ("From".to_owned(), format!("{sender} ({})", message.sender())),
        (
            "Kind".to_owned(),
            match &message.content {
                Content::Text { .. } => "text".to_owned(),
                Content::Sticker(_) => "sticker".to_owned(),
                Content::Poll(_) => "poll".to_owned(),
                Content::Deleted => "deleted".to_owned(),
                Content::Update(_) => "update".to_owned(),
            },
        ),
    ];

    if let Some(status) = message.status {
        fields.push(("Status".to_owned(), format!("{status:?}")));
    }
    if let Some(edited) = message.edited {
        fields.push(("Edited".to_owned(), edited.to_string()));
    }
    if let Some(quote) = message.quote.as_ref() {
        fields.push((
            "Quotes".to_owned(),
            format!("{} from {}", quote.id.timestamp, quote.id.sender),
        ));
    }
    if !message.ranges().is_empty() {
        fields.push((
            "Formatting".to_owned(),
            format!("{} range(s)", message.ranges().len()),
        ));
    }
    if !message.reactions.is_empty() {
        fields.push((
            "Reactions".to_owned(),
            message
                .reactions
                .iter()
                .map(|reaction| format!("{} {}", reaction.emoji, reaction.author))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    for attached in &message.attachments {
        fields.push((
            "Attachment".to_owned(),
            format!(
                "{} · {} · {} bytes · {}",
                attached.id.as_str(),
                attached.content_type,
                attached.size,
                match &attached.blob {
                    Blob::Cached(path) => path.display().to_string(),
                    Blob::Downloading(Some(fraction)) => {
                        format!("downloading {:.0}%", fraction * 100.0)
                    }
                    Blob::Downloading(None) => "downloading".to_owned(),
                    Blob::Missing => "not downloaded".to_owned(),
                    Blob::Failed(error) => format!("failed: {error}"),
                }
            ),
        ));
    }

    fields
}

impl gpui::Focusable for Raw {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Raw {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();

        let rows: Vec<_> = self
            .fields
            .iter()
            .map(|(name, value)| {
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .py_1()
                    .child(
                        div()
                            .flex_none()
                            .w(px(96.0))
                            .text_size(px(palette.typography.ui_size - 1.0))
                            .text_color(palette.text_muted)
                            .child(SharedString::from(name.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(palette.typography.ui_size))
                            .text_color(palette.text)
                            .child(SharedString::from(value.clone())),
                    )
            })
            .collect();

        kit::scrim(&palette)
            .id("raw")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                kit::dialog(560.0, &palette)
                    .max_h(px(560.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .text_size(px(palette.typography.ui_size + 2.0))
                            .text_color(palette.text)
                            .child("Message details"),
                    )
                    .child(div().flex().flex_col().children(rows))
                    .child(
                        div()
                            .id("raw-dump")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_2p5()
                            .rounded(px(kit::RADIUS))
                            .bg(palette.sunken)
                            .border_1()
                            .border_color(palette.border)
                            .font_family(palette.typography.mono.clone())
                            .text_size(px(palette.typography.ui_size - 2.0))
                            .text_color(palette.text_dim)
                            .child(SharedString::from(self.dump.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(kit::button(
                                "copy-raw",
                                "Copy",
                                kit::Intent::Quiet,
                                &palette,
                                cx.listener(|this: &mut Self, _, _, cx| this.copy(cx)),
                            ))
                            .child(kit::button(
                                "close-raw",
                                "Done",
                                kit::Intent::Primary,
                                &palette,
                                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
                            )),
                    ),
            )
    }
}
