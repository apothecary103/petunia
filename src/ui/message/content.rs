use gpui::prelude::*;
use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, MouseButton, SharedString, StyledText,
    div, px,
};
use gpui_component::IconName;

use super::act::{Act, Dispatch};
use super::{bar, emoji, format, media};
use crate::audio::Playback;
use crate::config::Theme;
use crate::config::messages::Spacing;
use crate::data::attachment::Blob;
use crate::data::message::{Content, Quote, Range, Status, Sticker, Update};
use crate::data::{Message, State};
use crate::ui::kit;

/// Everything one message shows: its body, whatever it carries, and whatever
/// was done to it afterwards.
pub struct Body<'a> {
    pub message: &'a Message,
    pub state: &'a State,
    pub theme: &'a Theme,
    pub spacing: Spacing,
    pub max_image: (f32, f32),
    pub playback: &'a Playback,
    /// The one way anything drawn on a message asks for something to happen.
    pub act: &'a Dispatch,
}

impl Body<'_> {
    pub fn render(self) -> Div {
        let theme = self.theme;
        let spacing = self.spacing;
        let own = self.message.sender() == self.state.aci;

        let mut block = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .when_some(self.message.quote.as_ref(), |this, quote| {
                this.child(quoted(quote, self.state, theme, spacing))
            });

        block = match &self.message.content {
            Content::Text { body, ranges } => {
                // A message that is nothing but a couple of emoji is drawn at a
                // size you can read, the way Signal does.
                let size = match emoji::jumbo(body) {
                    Some(scale) if ranges.is_empty() => spacing.body * scale,
                    _ => spacing.body,
                };
                block.child(
                    div()
                        .text_size(px(size))
                        .line_height(px(size * theme.typography.line_height))
                        .text_color(theme.text)
                        .child(styled(body, ranges, self.state, theme)),
                )
            }
            Content::Sticker(sticker) => block.child(self.sticker(sticker)),
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

        let frame = media::Frame {
            theme,
            spacing,
            max_image: self.max_image,
            timestamp: self.message.timestamp(),
            playback: self.playback,
            act: self.act,
        };
        for attached in &self.message.attachments {
            block = block.child(frame.render(attached));
        }

        if let Some(preview) = self.message.preview.as_ref() {
            block = block.child(link_card(preview, theme, spacing, self.act));
        }

        if !self.message.reactions.is_empty() {
            block = block.child(reactions(self.message, self.state, theme, self.act));
        }

        // Only our own messages have a delivery state, and only ours show one.
        if own && let Some(status) = self.message.status {
            block = block.child(receipt(status, self.message.edited.is_some(), theme));
        }

        bar::with_actions(block, self.message, own, theme, self.act)
    }

    /// A sticker has no bubble and no chip: a fixed square, and the pack's own
    /// emoji holding the space until the bytes arrive. Fixed rather than capped
    /// because a sticker that will not decode must not collapse to nothing.
    fn sticker(&self, sticker: &Sticker) -> AnyElement {
        let edge = self.spacing.sticker;
        let act = self.act.clone();
        let pack_id = sticker.pack_id.clone();
        let key = sticker.pack_key.clone();

        let square = div()
            .id(SharedString::from(format!("sticker-{}", sticker.sticker_id)))
            .size(px(edge))
            .flex()
            .flex_none()
            .items_center()
            .justify_center();

        // Clicking a sticker offers its pack, which is how you come by one.
        let square = match key {
            Some(key) => square
                .cursor_pointer()
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new("Add this sticker pack").build(window, cx)
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    act(
                        Act::InstallStickers {
                            pack_id: pack_id.clone(),
                            key: key.clone(),
                        },
                        window,
                        cx,
                    )
                }),
            None => square,
        };

        match sticker.image.as_ref().map(|image| &image.blob) {
            Some(Blob::Cached(path)) => square
                .child(crate::ui::image::picture(path, edge, edge))
                .into_any_element(),
            _ => square
                .text_size(px(edge * 0.35))
                .child(SharedString::from(
                    sticker.emoji.clone().unwrap_or_else(|| "🎨".into()),
                ))
                .into_any_element(),
        }
    }
}

/// How far a message of ours has got. Signal's own language: one tick sent, two
/// delivered, two in the accent colour read. Small and dim, because it matters
/// when you look for it and never otherwise.
fn receipt(status: Status, edited: bool, theme: &Theme) -> gpui::Stateful<Div> {
    let mark: AnyElement = match status {
        Status::Sending => kit::icon(IconName::Loader, 11.0, theme.text_muted).into_any_element(),
        Status::Failed => kit::icon(IconName::TriangleAlert, 11.0, theme.danger).into_any_element(),
        Status::Sent => ticks(1, theme.text_muted).into_any_element(),
        Status::Delivered => ticks(2, theme.text_muted).into_any_element(),
        Status::Read | Status::Viewed => ticks(2, theme.accent).into_any_element(),
    };
    let words = match status {
        Status::Sending => "Sending",
        Status::Failed => "Failed to send",
        Status::Sent => "Sent",
        Status::Delivered => "Delivered",
        Status::Read => "Read",
        Status::Viewed => "Viewed",
    };

    div()
        .id("receipt")
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(theme.typography.ui_size - 3.0))
        .text_color(theme.text_muted)
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(words).build(window, cx)
        })
        .when(edited, |this| this.child("edited"))
        .when(status == Status::Failed, |this| {
            this.text_color(theme.danger).child(words)
        })
        .child(mark)
}

/// Two ticks are one tick drawn twice, overlapped, because the icon set has no
/// double-tick and a second glyph beside the first reads as two separate marks.
fn ticks(count: usize, tint: gpui::Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .children((0..count).map(|index| {
            div()
                .when(index > 0, |this| this.ml(px(-4.0)))
                .child(kit::icon(IconName::Check, 11.0, tint))
        }))
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
                        .child(styled(&quote.body, &quote.ranges, state, theme)),
                ),
        )
}

/// The card a sender attached to a link.
///
/// Rendered only for what arrives: fetching the page ourselves to build one
/// would tell a third party the moment a link reached this client, which is
/// exactly what the sender's own preview exists to avoid.
fn link_card(
    preview: &crate::data::message::LinkPreview,
    theme: &Theme,
    spacing: Spacing,
    act: &Dispatch,
) -> gpui::Stateful<Div> {
    let act = act.clone();
    let url = preview.url.clone();
    let thumbnail = match preview.image.as_ref().map(|image| &image.blob) {
        Some(Blob::Cached(path)) => Some(path.clone()),
        _ => None,
    };

    div()
        .id("preview")
        .flex()
        .gap_2p5()
        .p_2()
        .max_w(px(360.0))
        .rounded(px(kit::RADIUS))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(|this| this.border_color(theme.border_focus))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            act(Act::OpenLink(url.clone()), window, cx)
        })
        .when_some(thumbnail, |this, path| {
            this.child(crate::ui::image::cropped(&path, 56.0).rounded(px(4.0)))
        })
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap_px()
                .when_some(preview.title.clone(), |this, title| {
                    this.child(
                        div()
                            .truncate()
                            .text_size(px(spacing.small + 1.0))
                            .text_color(theme.text)
                            .child(SharedString::from(title)),
                    )
                })
                .when_some(preview.description.clone(), |this, description| {
                    this.child(
                        div()
                            .truncate()
                            .text_size(px(spacing.small))
                            .text_color(theme.text_dim)
                            .child(SharedString::from(description)),
                    )
                })
                .child(
                    div()
                        .truncate()
                        .text_size(px(spacing.small))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(host(&preview.url))),
                ),
        )
}

/// A link shown as where it goes rather than as its full query string.
fn host(url: &str) -> String {
    url.rsplit("://")
        .next()
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .trim_start_matches("www.")
        .to_string()
}

/// One chip per distinct emoji, tinted when it includes you. Clicking a chip
/// adds your own reaction, or takes it back if it is already there.
fn reactions(message: &Message, state: &State, theme: &Theme, act: &Dispatch) -> Div {
    let mut counts: Vec<(String, Vec<String>, bool)> = Vec::new();

    for reaction in &message.reactions {
        let mine = reaction.author == state.aci;
        let who = state.sender_name(reaction.author);
        match counts
            .iter_mut()
            .find(|(emoji, _, _)| *emoji == reaction.emoji)
        {
            Some((_, names, ours)) => {
                names.push(who);
                *ours |= mine;
            }
            None => counts.push((reaction.emoji.clone(), vec![who], mine)),
        }
    }

    let id = message.id;

    div()
        .flex()
        .flex_wrap()
        .gap_1p5()
        .pt_0p5()
        .children(counts.into_iter().map(|(emoji, names, mine)| {
            let count = names.len();
            let who = SharedString::from(names.join(", "));
            let act = act.clone();
            let emoji_for_click = emoji.clone();

            div()
                .id(SharedString::from(format!("reaction-{emoji}-{}", id.timestamp)))
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_0p5()
                .rounded_full()
                .cursor_pointer()
                .bg(if mine {
                    kit::tinted(theme.accent)
                } else {
                    theme.elevated
                })
                .border_1()
                .border_color(if mine { theme.accent } else { theme.border })
                .text_size(px(theme.typography.ui_size - 2.0))
                .text_color(theme.text_dim)
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(who.clone()).build(window, cx)
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    act(Act::React(id, emoji_for_click.clone()), window, cx)
                })
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

