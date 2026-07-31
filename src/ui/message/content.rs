use gpui::prelude::*;
use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, MouseButton, SharedString, StyledText,
    div, img, px,
};
use gpui_component::IconName;

use crate::config::Theme;
use crate::config::messages::Spacing;
use crate::data::attachment::{Attachment, Blob, Kind};
use crate::data::message::{Content, Quote, Range, Reaction, Status, Update};
use crate::data::{Message, State};
use crate::ui::kit;
use super::format;

/// Everything one message shows: its body, whatever it carries, and whatever
/// was done to it afterwards.
pub struct Body<'a> {
    pub message: &'a Message,
    pub state: &'a State,
    pub theme: &'a Theme,
    pub spacing: Spacing,
    pub max_image: (f32, f32),
    /// Asks the worker for an attachment the auto-download policy skipped.
    pub on_download: crate::ui::conversation::Download,
}

impl Body<'_> {
    pub fn render(self) -> Div {
        let theme = self.theme;
        let spacing = self.spacing;

        let mut block = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .when_some(self.message.quote.as_ref(), |this, quote| {
                this.child(quoted(quote, self.state, theme, spacing))
            });

        block = match &self.message.content {
            Content::Text { body, ranges } => block.child(
                div()
                    .text_size(px(spacing.body))
                    .line_height(px(spacing.body * theme.typography.line_height))
                    .text_color(theme.text)
                    .child(styled(body, ranges, self.state, theme)),
            ),
            Content::Sticker(sticker) => block.child(self.sticker(sticker.image.as_ref())),
            Content::Deleted => block.child(
                div()
                    .text_size(px(spacing.body))
                    .text_color(theme.text_muted)
                    .italic()
                    .child("This message was deleted"),
            ),
            Content::Update(update) => block.child(
                div()
                    .text_size(px(spacing.small))
                    .text_color(theme.text_muted)
                    .child(SharedString::from(describe(update))),
            ),
        };

        for attachment in &self.message.attachments {
            block = block.child(self.attachment(attachment));
        }

        if !self.message.reactions.is_empty() {
            block = block.child(reactions(&self.message.reactions, self.state, theme));
        }

        // Only our own messages have a delivery state, and only ours show one.
        if self.message.sender() == self.state.aci
            && let Some(status) = self.message.status
        {
            block = block.child(receipt(status, self.message.edited.is_some(), theme));
        }

        block
    }

    fn sticker(&self, image: Option<&Attachment>) -> AnyElement {
        let edge = self.spacing.sticker;

        match image.map(|image| &image.blob) {
            Some(Blob::Cached(path)) => img(path.clone())
                .max_w(px(edge))
                .max_h(px(edge))
                .into_any_element(),
            _ => div()
                .size(px(edge * 0.5))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(edge * 0.35))
                .child("🎨")
                .into_any_element(),
        }
    }

    fn attachment(&self, attachment: &Attachment) -> AnyElement {
        let theme = self.theme;

        match (&attachment.kind, &attachment.blob) {
            // Sized explicitly rather than capped: an image's natural size is
            // its pixel size, and a max alone leaves the layout to guess the
            // other axis.
            (Kind::Image { size, .. }, Blob::Cached(path)) => {
                let (width, height) = fit(*size, self.max_image);
                img(path.clone())
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(kit::RADIUS))
                    .into_any_element()
            }
            (_, Blob::Cached(path)) => {
                file_chip(attachment, Some(path.to_string_lossy().into_owned()), theme)
                    .into_any_element()
            }
            (_, Blob::Downloading(progress)) => {
                progress_chip(attachment, *progress, theme).into_any_element()
            }
            (_, Blob::Failed(error)) => status_chip(
                attachment,
                format!("Could not download — {error}"),
                theme.danger,
                theme,
            )
            .into_any_element(),
            (_, Blob::Missing) => {
                let id = attachment.id.clone();
                let timestamp = self.message.timestamp();
                let ask = self.on_download.clone();
                file_chip(attachment, None, theme)
                    .id(SharedString::from(format!("get-{}", id.as_str())))
                    .cursor_pointer()
                    .hover(|this| this.border_color(theme.border_focus))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        ask(timestamp, &id, window, cx)
                    })
                    .into_any_element()
            }
        }
    }
}

/// How far a message of ours has got. Small and dim: it matters when you look
/// for it and never otherwise.
fn receipt(status: Status, edited: bool, theme: &Theme) -> Div {
    let (label, tint) = match status {
        Status::Sending => ("Sending…", theme.text_muted),
        Status::Failed => ("Failed to send", theme.danger),
        Status::Sent => ("Sent", theme.text_muted),
        Status::Delivered => ("Delivered", theme.text_muted),
        Status::Read => ("Read", theme.text_dim),
        Status::Viewed => ("Viewed", theme.text_dim),
    };

    div()
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(theme.typography.ui_size - 3.0))
        .text_color(tint)
        .when(edited, |this| {
            this.child(
                div()
                    .text_color(theme.text_muted)
                    .child("edited")
                    .into_any_element(),
            )
        })
        .child(label)
}

/// Renders the body with Signal's formatting applied. Mentions carry a
/// placeholder in the body, so the name is substituted before highlighting and
/// the offsets are recomputed against the text actually drawn.
fn styled(body: &str, ranges: &[Range], state: &State, theme: &Theme) -> StyledText {
    let segments = format::segments(body, ranges);
    let mut text = String::new();
    let mut highlights = Vec::new();

    for segment in segments {
        let styles = segment.styles;
        let start = text.len();

        match (styles.spoiler, styles.mention) {
            // A hidden spoiler must not leak its text through glyph widths, so
            // it is replaced rather than merely recoloured.
            (true, _) => {
                let width = body[segment.start..segment.end].chars().count();
                text.push_str(&"█".repeat(width.clamp(1, 40)));
            }
            (false, Some(uuid)) => {
                text.push('@');
                text.push_str(&state.sender_name(uuid));
            }
            (false, None) => text.push_str(&body[segment.start..segment.end]),
        }

        if let Some(highlight) = highlight(styles, theme) {
            highlights.push((start..text.len(), highlight));
        }
    }

    StyledText::new(text).with_highlights(highlights)
}

fn highlight(styles: format::Styles, theme: &Theme) -> Option<HighlightStyle> {
    let mut highlight = HighlightStyle::default();
    let mut touched = false;

    if styles.bold {
        highlight.font_weight = Some(FontWeight::BOLD);
        touched = true;
    }
    if styles.italic {
        highlight.font_style = Some(FontStyle::Italic);
        touched = true;
    }
    if styles.strikethrough {
        highlight.strikethrough = Some(gpui::StrikethroughStyle {
            thickness: px(1.0),
            color: Some(theme.text_dim),
        });
        touched = true;
    }
    if styles.monospace {
        highlight.background_color = Some(theme.sunken);
        touched = true;
    }
    if styles.spoiler {
        // Same colour as the block it draws, so nothing shows through until a
        // reveal replaces the text.
        highlight.color = Some(theme.text_muted);
        highlight.background_color = Some(theme.text_muted);
        touched = true;
    } else if styles.mention.is_some() {
        highlight.color = Some(theme.accent);
        highlight.background_color = Some(kit::tinted(theme.accent));
        touched = true;
    } else if styles.link {
        highlight.color = Some(theme.accent);
        highlight.underline = Some(gpui::UnderlineStyle {
            thickness: px(1.0),
            color: Some(theme.accent),
            wavy: false,
        });
        touched = true;
    }

    touched.then_some(highlight)
}

/// Scales an image down to fit inside the box, keeping its aspect ratio and
/// never scaling up. Without the pixel size to work from, the box is all we
/// can honour.
fn fit(size: Option<crate::data::attachment::Size>, max: (f32, f32)) -> (f32, f32) {
    let Some(size) = size.filter(|size| size.width > 0 && size.height > 0) else {
        return max;
    };

    let (width, height) = (size.width as f32, size.height as f32);
    let scale = (max.0 / width).min(max.1 / height).min(1.0);

    (width * scale, height * scale)
}

fn icon_for(kind: &Kind) -> IconName {
    match kind {
        Kind::Image { .. } => IconName::Frame,
        Kind::Video { .. } => IconName::Play,
        Kind::Audio { .. } => IconName::Bell,
        Kind::File => IconName::File,
    }
}

fn label(attachment: &Attachment) -> String {
    attachment
        .file_name
        .clone()
        .unwrap_or_else(|| match attachment.kind {
            Kind::Image { .. } => "Photo".into(),
            Kind::Video { .. } => "Video".into(),
            Kind::Audio { voice_note: true, .. } => "Voice message".into(),
            Kind::Audio { .. } => "Audio".into(),
            Kind::File => "File".into(),
        })
}

/// Bytes as something a person reads, not as a number of bytes.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn chip_shell(theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2p5()
        .px_3()
        .py_2()
        .rounded(px(kit::RADIUS))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border)
}

fn file_chip(attachment: &Attachment, cached: Option<String>, theme: &Theme) -> gpui::Stateful<Div> {
    let detail = match cached {
        Some(_) => size(attachment.size),
        None => format!("{} · tap to download", size(attachment.size)),
    };

    chip_shell(theme)
        .id("chip")
        .child(kit::icon(icon_for(&attachment.kind), 16.0, theme.text_dim))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(theme.typography.ui_size))
                        .text_color(theme.text)
                        .child(SharedString::from(label(attachment))),
                )
                .child(
                    div()
                        .text_size(px(theme.typography.ui_size - 3.0))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(detail)),
                ),
        )
}

fn progress_chip(attachment: &Attachment, progress: f32, theme: &Theme) -> Div {
    status_chip(
        attachment,
        format!("Downloading… {:.0}%", progress * 100.0),
        theme.text_muted,
        theme,
    )
}

fn status_chip(
    attachment: &Attachment,
    detail: String,
    tint: gpui::Hsla,
    theme: &Theme,
) -> Div {
    chip_shell(theme)
        .child(kit::icon(icon_for(&attachment.kind), 16.0, tint))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(theme.typography.ui_size))
                        .text_color(theme.text)
                        .child(SharedString::from(label(attachment))),
                )
                .child(
                    div()
                        .text_size(px(theme.typography.ui_size - 3.0))
                        .text_color(tint)
                        .child(SharedString::from(detail)),
                ),
        )
}

/// The message being answered, as a bar rather than a box: it is context, and
/// context should not outweigh the reply.
fn quoted(quote: &Quote, state: &State, theme: &Theme, spacing: Spacing) -> Div {
    let author = state.sender_name(quote.id.sender);
    let tint = theme.accent_for(quote.id.sender.as_bytes());

    div()
        .flex()
        .gap_2p5()
        .child(div().w_px().flex_none().bg(tint).rounded_full())
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap_px()
                .child(
                    div()
                        .text_size(px(spacing.small))
                        .text_color(tint)
                        .child(SharedString::from(author)),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(spacing.small))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(quote.body.clone())),
                ),
        )
}

fn reactions(reactions: &[Reaction], state: &State, theme: &Theme) -> Div {
    let mut counts: Vec<(String, usize, bool)> = Vec::new();

    for reaction in reactions {
        let mine = reaction.author == state.aci;
        match counts.iter_mut().find(|(emoji, _, _)| *emoji == reaction.emoji) {
            Some((_, count, ours)) => {
                *count += 1;
                *ours |= mine;
            }
            None => counts.push((reaction.emoji.clone(), 1, mine)),
        }
    }

    div()
        .flex()
        .flex_wrap()
        .gap_1p5()
        .pt_0p5()
        .children(counts.into_iter().map(|(emoji, count, mine)| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(if mine {
                    kit::tinted(theme.accent)
                } else {
                    theme.elevated
                })
                .border_1()
                .border_color(if mine { theme.accent } else { theme.border })
                .text_size(px(theme.typography.ui_size - 2.0))
                .text_color(theme.text_dim)
                .child(SharedString::from(emoji))
                .when(count > 1, |this| {
                    this.child(SharedString::from(count.to_string()))
                })
        }))
}

fn describe(update: &Update) -> String {
    match update {
        Update::ExpireTimer { seconds: 0 } => "Disappearing messages off".into(),
        Update::ExpireTimer { .. } => "Disappearing messages on".into(),
    }
}

/// A download is asked for by clicking the chip, so the chip has to know which
/// attachment it is.
pub fn wants_download(attachment: &Attachment) -> bool {
    matches!(attachment.blob, Blob::Missing | Blob::Failed(_))
}

pub fn clickable(element: Div, on_click: impl Fn() + 'static) -> Div {
    element.on_mouse_down(MouseButton::Left, move |_, _, _| on_click())
}
