use gpui::prelude::*;
use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, MouseButton, SharedString, StyledText,
    div, px,
};
use gpui_component::IconName;

use super::act::{Act, Dispatch};
use super::{bar, emoji, format, media};
use crate::data::message::markup;
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
    pub fn render(self) -> gpui::Stateful<Div> {
        let theme = self.theme;
        let spacing = self.spacing;
        let own = self.message.sender() == self.state.aci;

        let mut said = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .when_some(self.message.quote.as_ref(), |this, quote| {
                this.child(quoted(quote, self.state, theme, spacing))
            });

        said = match &self.message.content {
            Content::Text { body, ranges } => {
                // No client sends markdown as ranges, so a message typed as
                // `*hai*` elsewhere arrives as three literal characters. It is
                // read here -- but only when the sender said nothing, because a
                // sender who did send ranges meant them, and a second pass over
                // the same text would fight their offsets.
                let (body, ranges) = match ranges.is_empty() {
                    true => {
                        let (read, found) = markup::parse(body);
                        (std::borrow::Cow::Owned(read), std::borrow::Cow::Owned(found))
                    }
                    false => (
                        std::borrow::Cow::Borrowed(body.as_str()),
                        std::borrow::Cow::Borrowed(ranges.as_slice()),
                    ),
                };

                // A message that is nothing but a couple of emoji is drawn at a
                // size you can read, the way Signal does.
                let size = match emoji::jumbo(&body) {
                    Some(scale) if ranges.is_empty() => spacing.body * scale,
                    _ => spacing.body,
                };
                said.children(prose(&body, &ranges, self.state, theme, size))
            }
            Content::Sticker(sticker) => said.child(self.sticker(sticker)),
            Content::Deleted => said.child(
                div()
                    .text_size(px(spacing.body))
                    .text_color(theme.text_muted)
                    .italic()
                    .child("This message was deleted"),
            ),
            Content::Update(update) => said.child(
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
            said = said.child(frame.render(attached));
        }

        if let Some(preview) = self.message.preview.as_ref() {
            said = said.child(link_card(preview, theme, spacing, self.act));
        }

        // The mark trails what was said rather than taking a line of its own:
        // one line per message of "Read" doubles the height of a conversation
        // to say something you only look for when you look for it.
        let mark = (own && self.message.status.is_some()).then(|| {
            receipt(
                self.message.status.expect("checked"),
                self.message.edited.is_some(),
                theme,
            )
        });

        let mut block = div().flex().flex_col().gap_1p5().child(
            div()
                .flex()
                .items_end()
                .gap_2()
                .child(said.min_w_0())
                .children(mark),
        );

        if !self.message.reactions.is_empty() {
            block = block.child(reactions(self.message, self.state, theme, self.act));
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
        .flex_none()
        .items_center()
        .gap_1()
        // Lifted off the baseline so the tick sits with the text rather than
        // hanging below it.
        .pb(px(2.0))
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

/// The body, split so that a code block gets a box of its own and everything
/// else stays in the paragraph it belongs to.
///
/// A block is a monospace range covering whole lines, which is the only thing
/// the wire can say about it: Signal has one monospace style and no way to mark
/// a block as such.
fn prose(
    body: &str,
    ranges: &[Range],
    state: &State,
    theme: &Theme,
    size: f32,
) -> Vec<AnyElement> {
    let mut blocks: Vec<&Range> = ranges
        .iter()
        .filter(|range| markup::is_block(body, range))
        .collect();
    blocks.sort_by_key(|range| range.start);

    let paragraph = |from: usize, to: usize| {
        (to > from).then(|| {
            let text = body[from..to].trim_matches('\n');
            (!text.is_empty()).then(|| {
                let shifted: Vec<Range> = ranges
                    .iter()
                    .filter(|range| range.start >= from && range.end() <= to)
                    .map(|range| Range {
                        start: range.start - from,
                        ..*range
                    })
                    .collect();
                let offset = body[from..to].find(text).unwrap_or(0);
                let shifted = shifted
                    .into_iter()
                    .filter_map(|range| {
                        range.start.checked_sub(offset).map(|start| Range { start, ..range })
                    })
                    .collect::<Vec<_>>();

                div()
                    .text_size(px(size))
                    .line_height(px(size * theme.typography.line_height))
                    .text_color(theme.text)
                    .child(styled(text, &shifted, state, theme))
                    .into_any_element()
            })
        })
    };

    if blocks.is_empty() {
        return paragraph(0, body.len()).flatten().into_iter().collect();
    }

    let mut parts = Vec::new();
    let mut at = 0;
    for block in blocks {
        parts.extend(paragraph(at, block.start).flatten());
        parts.push(code_block(&body[block.start..block.end()], theme, size));
        at = block.end();
    }
    parts.extend(paragraph(at, body.len()).flatten());
    parts
}

/// Code, in a box, in the monospace font, coloured by what it is. Nothing else
/// in a message gets a background, which is what makes it read as a block
/// rather than as a word.
fn code_block(fenced: &str, theme: &Theme, size: f32) -> AnyElement {
    let Some((language, code)) = markup::block(fenced) else {
        return div().into_any_element();
    };
    let language = language.unwrap_or("text");

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .rounded(px(kit::RADIUS))
        .bg(theme.sunken)
        .border_1()
        .border_color(theme.border)
        .when(language != "text", |this| {
            this.child(
                div()
                    .text_size(px(size - 3.0))
                    .text_color(theme.text_muted)
                    .child(SharedString::from(language.to_owned())),
            )
        })
        .child(
            div()
                .font_family(theme.typography.mono.clone())
                .text_size(px(size - 1.0))
                .line_height(px((size - 1.0) * 1.45))
                .text_color(theme.text)
                .child(highlighted(code, language, theme)),
        )
        .into_any_element()
}

/// Runs the code through the widget library's tree-sitter highlighter. It has a
/// grammar for the languages people actually paste; anything else parses as
/// text and comes back one flat span, which is the right answer rather than a
/// guess dressed up in colour.
fn highlighted(code: &str, language: &str, theme: &Theme) -> StyledText {
    use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};

    let mut highlighter = SyntaxHighlighter::new(language);
    let rope = gpui_component::input::Rope::from(code);
    highlighter.update(None, &rope, None);

    let palette = match theme.is_light() {
        true => HighlightTheme::default_light(),
        false => HighlightTheme::default_dark(),
    };
    let styles: Vec<_> = highlighter
        .styles(&(0..code.len()), &palette)
        .into_iter()
        .filter(|(range, _)| range.end <= code.len() && code.is_char_boundary(range.start))
        .collect();

    StyledText::new(code.to_owned()).with_highlights(styles)
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

