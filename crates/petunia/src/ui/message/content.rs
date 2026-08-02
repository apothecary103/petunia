use gpui::prelude::*;
use gpui::{
    AnyElement, Div, FontStyle, HighlightStyle, MouseButton, SharedString, StyledText,
    div, px,
};
use gpui_component::IconName;
use gpui_component::highlighter::HighlightTheme;

use super::act::{Act, Dispatch};
use super::{bar, emoji, format, media};
use petunia_data::message::{latex, markup};
use petunia_media::audio::Playback;
use petunia_config::Theme;
use petunia_config::messages::{Reply, Spacing};
use petunia_data::attachment::Blob;
use petunia_data::message::{Content, Poll, Quote, Range, Status, Sticker, Update};
use petunia_data::{Message, State};
use crate::ui::image;
use crate::ui::kit;

/// Everything one message shows: its body, whatever it carries, and whatever
/// was done to it afterwards.
pub struct Body<'a> {
    pub message: &'a Message,
    pub state: &'a State,
    pub theme: &'a Theme,
    /// Derived once when the theme is installed rather than here: building it is
    /// a round trip through Zed's theme JSON, and this runs per frame.
    pub highlights: &'a HighlightTheme,
    pub spacing: Spacing,
    /// What shape the message being answered is drawn in.
    pub replies: Reply,
    pub max_image: (f32, f32),
    pub playback: &'a Playback,
    /// Whether this message is the one that carries the receipt. Not every
    /// message of ours does: see `Run::marked`.
    pub marked: bool,
    /// Whether its text has just been copied, which the bar says for a moment.
    pub copied: bool,
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
                this.child(quoted(quote, self.state, theme, spacing, self.replies))
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
                said.children(prose(
                    &body,
                    &ranges,
                    self.state,
                    theme,
                    self.highlights,
                    size,
                    self.message.timestamp(),
                ))
            }
            Content::Sticker(sticker) => said.child(self.sticker(sticker)),
            Content::Poll(poll) => said.child(self.poll(poll, own)),
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
            highlights: self.highlights,
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
        //
        // "Edited" is not a receipt and is drawn wherever it applies: a layout
        // that carries the tick on one message only must still say which messages
        // were changed after the fact.
        let status = (own && self.marked).then_some(self.message.status).flatten();
        let edited = self.message.edited.is_some();
        let mark = (status.is_some() || edited).then(|| receipt(status, edited, theme));

        // A message that is going to disappear says so, beside the mark and in
        // the same grey. Not a countdown: the clock started when it was read
        // and a second-by-second number on every message in a thread is a
        // thread that never stops repainting — what matters is *that* this one
        // is temporary, and for how long it was given.
        let expiring = self
            .message
            .expires_in
            .filter(|seconds| *seconds > 0)
            .map(|seconds| self.expiry(seconds, theme));

        let mut block = div().flex().flex_col().gap_1p5().child(
            div()
                .flex()
                .items_end()
                .gap_2()
                .child(said.min_w_0())
                .children(expiring)
                .children(mark),
        );

        if !self.message.reactions.is_empty() {
            block = block.child(reactions(self.message, self.state, theme, self.act));
        }

        bar::with_actions(block, self.message, own, self.copied, theme, self.act)
    }

    /// The question, then one row per option: a bar filled to its share of the
    /// vote, the count, and a check on whatever this reader chose. Closed once
    /// the poll's own author says so, after which nothing here is clickable.
    fn poll(&self, poll: &Poll, own: bool) -> AnyElement {
        let theme = self.theme;
        let spacing = self.spacing;
        let id = self.message.id;
        let act = self.act.clone();
        let total = poll.ballots.len().max(1);
        let mine = poll
            .ballot_for(self.state.aci)
            .map(|ballot| ballot.option_indexes.clone())
            .unwrap_or_default();

        let mut column = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .w(px(280.0))
            .child(
                div()
                    .text_size(px(spacing.body))
                    .font_weight(kit::EMPHASIS)
                    .text_color(theme.text)
                    .child(poll.question.clone()),
            );

        for (index, option) in poll.options.iter().enumerate() {
            let votes = poll.votes_for(index);
            let share = votes as f32 / total as f32;
            let checked = mine.contains(&(index as u32));
            let act = act.clone();
            let chosen = match poll.allow_multiple {
                true if checked => mine.iter().copied().filter(|&i| i != index as u32).collect(),
                true => {
                    let mut chosen = mine.clone();
                    chosen.push(index as u32);
                    chosen
                }
                false if checked => Vec::new(),
                false => vec![index as u32],
            };

            let mut row = div()
                .id(SharedString::from(format!("poll-{}-{index}", id.timestamp)))
                .relative()
                .flex()
                .items_center()
                .gap_2()
                .px_2p5()
                .py_1p5()
                .rounded(px(kit::RADIUS))
                .border_1()
                .border_color(match checked {
                    true => theme.accent,
                    false => theme.border,
                })
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(kit::RADIUS))
                        .bg(kit::tinted(theme.accent))
                        .w(gpui::relative(share)),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(spacing.body))
                        .text_color(theme.text)
                        .child(option.clone()),
                )
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .text_size(px(spacing.small))
                        .text_color(theme.text_muted)
                        .child(votes.to_string()),
                );

            if !poll.terminated {
                row = row.cursor_pointer().on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    act(Act::VotePoll(id, chosen.clone()), window, cx)
                });
            }
            column = column.child(row);
        }

        column = column.child(
            div()
                .text_size(px(spacing.small))
                .text_color(theme.text_muted)
                .child(match poll.terminated {
                    true => "Poll closed".to_string(),
                    false if poll.allow_multiple => "Select one or more".to_string(),
                    false => "Select one".to_string(),
                }),
        );

        if own && !poll.terminated {
            column = column.child(
                div()
                    .id(SharedString::from(format!("poll-end-{}", id.timestamp)))
                    .cursor_pointer()
                    .text_size(px(spacing.small))
                    .text_color(theme.danger)
                    .hover(|this| this.underline())
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        act(Act::TerminatePoll(id), window, cx)
                    })
                    .child("End this poll"),
            );
        }

        column.into_any_element()
    }

    /// A sticker has no bubble and no chip: a fixed square, and the pack's own
    /// emoji holding the space until the bytes arrive. Fixed rather than capped
    /// because a sticker that will not decode must not collapse to nothing.
    fn sticker(&self, sticker: &Sticker) -> AnyElement {
        let edge = self.spacing.sticker;
        let act = self.act.clone();
        let opened = Box::new(sticker.clone());

        // Clicking a sticker opens it, the way it does on the phone: the picture
        // at a size worth looking at, and the pack behind it. Installing is one
        // of the things offered there rather than what the click itself does.
        let square = div()
            .id(SharedString::from(format!("sticker-{}", sticker.sticker_id)))
            .size(px(edge))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                act(Act::ShowSticker(opened.clone()), window, cx)
            });

        match sticker.image.as_ref().map(|image| &image.blob) {
            Some(Blob::Cached(path)) => square
                .child(crate::ui::image::animated("frames", path, edge, edge))
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

impl Body<'_> {
    /// The mark on a message that is going away: a small clock and how long it
    /// was given. Beside the receipt rather than under the message, because it
    /// is the same kind of annotation and belongs in the same corner.
    fn expiry(&self, seconds: u32, theme: &Theme) -> gpui::Stateful<Div> {
        let label = crate::ui::details::timer_label(std::time::Duration::from_secs(seconds.into()));

        div()
            .id(SharedString::from(format!(
                "expiry-{}",
                self.message.timestamp()
            )))
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new("Disappears after it has been read")
                    .build(window, cx)
            })
            .child(kit::icon(IconName::CircleCheck, 10.0, theme.text_muted))
            .child(
                div()
                    .text_size(px(self.spacing.small))
                    .text_color(theme.text_muted)
                    .child(SharedString::from(label)),
            )
    }
}

/// How far a message of ours has got, in Signal's own language: a circled check
/// sent, a second circle delivered, both filled read. Small and dim, because it
/// matters when you look for it and never otherwise.
fn receipt(status: Option<Status>, edited: bool, theme: &Theme) -> gpui::Stateful<Div> {
    /// How large the mark is beside a message's clock.
    const TICK: f32 = 11.0;

    let mark = status.map(|status| -> AnyElement {
        match status {
            Status::Sending => {
                kit::icon(IconName::Loader, 11.0, theme.text_muted).into_any_element()
            }
            Status::Failed => {
                kit::icon(IconName::TriangleAlert, 11.0, theme.danger).into_any_element()
            }
            Status::Sent => kit::receipt(kit::Mark::Sent, TICK, theme.text_muted).into_any_element(),
            Status::Delivered => {
                kit::receipt(kit::Mark::Delivered, TICK, theme.text_muted).into_any_element()
            }
            Status::Read | Status::Viewed => {
                kit::receipt(kit::Mark::Read, TICK, theme.text_muted).into_any_element()
            }
        }
    });
    let words = match status {
        Some(Status::Sending) => "Sending",
        Some(Status::Failed) => "Failed to send",
        Some(Status::Sent) => "Sent",
        Some(Status::Delivered) => "Delivered",
        Some(Status::Read) => "Read",
        Some(Status::Viewed) => "Viewed",
        None => "Edited after it was sent",
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
        .when(status == Some(Status::Failed), |this| {
            this.text_color(theme.danger).child(words)
        })
        .children(mark)
}

/// The body, split so that a code block gets a box of its own and everything
/// else stays in the paragraph it belongs to.
///
/// A code block is a monospace range covering whole lines, which is the only
/// thing the wire can say about it: Signal has one monospace style and no way to
/// mark a block as such. Display maths is `$$…$$`, which is the source saying so
/// itself.
fn prose(
    body: &str,
    ranges: &[Range],
    state: &State,
    theme: &Theme,
    highlights: &HighlightTheme,
    size: f32,
    // What this message's runs of text are called while one of them is selected,
    // which is the timestamp it was said at. See `ui::selection`.
    said: u64,
) -> Vec<AnyElement> {
    let mut blocks = blocks(body, ranges);
    blocks.sort_by_key(|(start, _, _)| *start);

    let paragraph = |from: usize, to: usize| {
        (to > from).then(|| {
            let text = body[from..to].trim_matches('\n');
            (!text.is_empty()).then(|| {
                // Two shifts in one pass: out of the whole body and past the
                // newlines the trim took off the front. A range that started
                // inside those is not in this paragraph any more.
                let offset = from + body[from..to].find(text).unwrap_or(0);
                let shifted: Vec<Range> = ranges
                    .iter()
                    .filter(|range| range.start >= from && range.end() <= to)
                    .filter_map(|range| {
                        range.start.checked_sub(offset).map(|start| Range {
                            start,
                            ..*range
                        })
                    })
                    .collect();

                div()
                    .text_size(px(size))
                    .line_height(px(size * theme.typography.line_height))
                    .text_color(theme.text)
                    .child(styled(
                        text,
                        &shifted,
                        state,
                        theme,
                        // Where the paragraph starts, which is what tells two
                        // paragraphs of one message apart and stays the same
                        // frame to frame.
                        SharedString::from(format!("{said}-{from}")),
                        true,
                    ))
                    .into_any_element()
            })
        })
    };

    if blocks.is_empty() {
        return paragraph(0, body.len()).flatten().into_iter().collect();
    }

    let mut parts = Vec::new();
    let mut at = 0;
    for (start, end, block) in blocks {
        parts.extend(paragraph(at, start).flatten());
        parts.push(match block {
            Block::Code => code_block(&body[start..end], theme, highlights, size),
            Block::Maths(tex) => maths_block(&tex, theme, size),
        });
        at = end;
    }
    parts.extend(paragraph(at, body.len()).flatten());
    parts
}

/// What comes out of the paragraph it was written in and gets an element of its
/// own.
enum Block {
    Code,
    /// The source, already cut out of the body: unlike a fence, the delimiters are
    /// not wanted and the reading is not the characters that were typed.
    Maths(String),
}

/// Everything in the body that is not prose, as byte ranges over it.
///
/// Maths inside a code block is code -- a listing that happens to contain two
/// dollars is a listing -- so an overlap is resolved in the block's favour.
fn blocks(body: &str, ranges: &[Range]) -> Vec<(usize, usize, Block)> {
    let code: Vec<(usize, usize)> = ranges
        .iter()
        .filter(|range| markup::is_block(body, range))
        .map(|range| (range.start, range.end()))
        .collect();

    let maths = latex::spans(body)
        .into_iter()
        .filter(|span| span.kind == latex::Kind::Display)
        .filter(|span| {
            !code
                .iter()
                .any(|(start, end)| span.start < *end && *start < span.end)
        })
        .map(|span| (span.start, span.end, Block::Maths(span.tex)));

    code.iter()
        .map(|(start, end)| (*start, *end, Block::Code))
        .chain(maths)
        .collect()
}

/// A display equation, on a line of its own and set larger than the words around
/// it -- which is the whole of what "display" means, and the one place the size
/// *can* differ: a text run carries no size of its own in gpui, so maths inside a
/// sentence is stuck at the sentence's size and this is not.
///
/// Left-aligned rather than centred. Centring is what a page does, and a page has
/// a measure to centre within; a bubble is as wide as its widest line, so centring
/// an equation in one either does nothing or moves it away from the text it
/// belongs to.
fn maths_block(tex: &str, theme: &Theme, size: f32) -> AnyElement {
    let read = latex::parse(tex);
    if read.is_empty() {
        return div().into_any_element();
    }

    // Elements rather than a text run, which is what a display equation is for:
    // out of the paragraph, it can have a numerator over a denominator, a rule
    // between them, and limits stacked on a ∑. Inline maths cannot -- a line
    // that wraps has to be one text run -- and keeps the reading.
    div()
        .py_1()
        .font_family(theme.typography.serif.clone())
        .child(super::maths::typeset(
            &read,
            theme,
            size * DISPLAY_MATHS,
        ))
        .into_any_element()
}

/// Code, in a box, in the monospace font, coloured by what it is. Nothing else
/// in a message gets a background, which is what makes it read as a block
/// rather than as a word.
///
/// The bar across the top is what every place code is quoted has: the language
/// on the left, and on the right the one thing anybody wants from a listing
/// somebody else pasted, which is to have it. It is drawn whether or not the
/// fence declared a language, because the button is the reason it is there.
fn code_block(
    fenced: &str,
    theme: &Theme,
    highlights: &HighlightTheme,
    size: f32,
) -> AnyElement {
    let Some((language, code)) = markup::block(fenced) else {
        return div().into_any_element();
    };
    let named = language;
    let language = language.unwrap_or("text");
    let copied = code.to_owned();

    box_of_code(theme)
        .child(
            bar_of_code(theme)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(size - 3.0))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(named.unwrap_or_default().to_owned())),
                )
                .child(kit::icon_button(
                    SharedString::from(format!("copy-code-{:x}", fnv(fenced))),
                    IconName::Copy,
                    theme,
                    move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()))
                    },
                )),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .child(lines(code, language, theme, highlights, size)),
        )
        .into_any_element()
}

/// An element id for a block of code, which has nothing else to be named by:
/// two identical listings in one message are the same code and may share one.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

/// The box a block of code sits in. Shared with the attachment preview, which is
/// the same thing arriving as a file rather than as words -- so the padding
/// belongs to what goes inside rather than to this, since a block with a bar
/// across the top needs the bar to reach both edges.
pub fn box_of_code(theme: &Theme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(kit::RADIUS))
        .bg(theme.sunken)
        .border_1()
        .border_color(theme.border)
}

/// The strip across the top of a box of code: what it is on the left, and the
/// one thing to be done with it on the right.
///
/// Shared with the attachment preview, which is the same box with a file's name
/// and icon in the strip where a listing has its language -- a previewed text
/// file and a pasted listing are the same object arriving two ways, and two
/// headers for them would be two shapes for one thing.
pub fn bar_of_code(theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .bg(theme.elevated)
        .border_b_1()
        .border_color(theme.border)
}

/// Code as it is drawn: monospace, at the message's own size, coloured by what
/// the highlighter made of it.
pub fn lines(
    code: &str,
    language: &str,
    theme: &Theme,
    highlights: &HighlightTheme,
    size: f32,
) -> Div {
    div()
        .font_family(theme.typography.mono.clone())
        .text_size(px(size - 1.0))
        .line_height(px((size - 1.0) * 1.45))
        .text_color(theme.text)
        .child(highlighted(code, language, highlights))
}

/// Runs the code through the widget library's tree-sitter highlighter. It has a
/// grammar for the languages people actually paste; anything else parses as
/// text and comes back one flat span, which is the right answer rather than a
/// guess dressed up in colour.
fn highlighted(code: &str, language: &str, highlights: &HighlightTheme) -> StyledText {
    let styles = memoized(code, language, highlights);
    StyledText::new(code.to_owned()).with_highlights(styles.iter().cloned())
}

type Styles = std::rc::Rc<Vec<(std::ops::Range<usize>, HighlightStyle)>>;

/// A parse costs the better part of a millisecond and the answer never changes,
/// but a code block on screen is re-rendered on every frame -- so highlighting
/// one was a millisecond of every frame it was visible, for a result identical to
/// the last.
///
/// Bounded rather than unbounded: a long session scrolling through a thread full
/// of code would otherwise keep every block it had ever drawn. Cleared wholesale
/// when the theme changes, since every entry's colours came from it.
fn memoized(code: &str, language: &str, highlights: &HighlightTheme) -> Styles {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};

    /// Enough for what is on screen and the overdraw around it, several times
    /// over.
    const CAPACITY: usize = 64;

    /// The theme the entries were coloured with, and the entries by language and
    /// source.
    type Cache = (u64, HashMap<(String, String), Styles>);

    thread_local! {
        static CACHE: RefCell<Cache> = RefCell::new((0, HashMap::new()));
    }

    let theme = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        highlights.hash(&mut hasher);
        hasher.finish()
    };

    CACHE.with(|cache| {
        let (cached_theme, entries) = &mut *cache.borrow_mut();
        if *cached_theme != theme {
            entries.clear();
            *cached_theme = theme;
        }

        let key = (language.to_owned(), code.to_owned());
        if let Some(styles) = entries.get(&key) {
            return styles.clone();
        }

        let styles: Styles = std::rc::Rc::new(parse(code, language, highlights));
        if entries.len() >= CAPACITY {
            entries.clear();
        }
        entries.insert(key, styles.clone());
        styles
    })
}

fn parse(
    code: &str,
    language: &str,
    highlights: &HighlightTheme,
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    use gpui_component::highlighter::SyntaxHighlighter;

    let mut highlighter = SyntaxHighlighter::new(language);
    let rope = gpui_component::input::Rope::from(code);
    highlighter.update(None, &rope, None);

    highlighter
        .styles(&(0..code.len()), highlights)
        .into_iter()
        .filter(|(range, _)| range.end <= code.len() && code.is_char_boundary(range.start))
        .collect()
}

/// How much larger a display equation is set than the words around it.
///
/// A serif at the body size reads *smaller* than the interface face beside it --
/// its x-height is lower for the same nominal size -- so parity here would be a
/// visible step down, which is the opposite of what `$$` asks for. A third again
/// puts an equation a little above the text, which is where a display equation
/// sits on a page.
const DISPLAY_MATHS: f32 = 1.35;

/// How round inline code is. Half the block's, because the same radius on a box
/// the height of one line turns a chip into a lozenge.
const INLINE_RADIUS: f32 = kit::RADIUS / 2.0;

/// How far the chip reaches past the code inside it, left and right. Wider than
/// the fifth of an em a wash needs: a box with a hairline around it has to hold
/// the code clear of its own edge, the way the fenced block does.
const INLINE_PAD: f32 = 4.0;

/// Renders the body with Signal's formatting applied. Mentions carry a
/// placeholder in the body, so the name is substituted before highlighting and
/// the offsets are recomputed against the text actually drawn. So does maths,
/// which is read out of the source the sender typed and drawn as the symbols it
/// spells.
///
/// Inline code comes back as the same object the fenced block is, only the size
/// of a word: the block's fill, the block's hairline, and half the block's
/// radius, since the full one on a box a single line tall is a lozenge rather
/// than a chip. Anything less than the border made `` `bat` `` read as a
/// highlighter pen over monospace rather than as code. See `ui::wash` for why
/// none of this can be a highlight, and for why the padding is the box's rather
/// than two thin spaces nobody typed.
fn styled(
    body: &str,
    ranges: &[Range],
    state: &State,
    theme: &Theme,
    // What this run of text is called: half of what says which spoiler in it has
    // been uncovered, and which run a selection belongs to.
    id: SharedString,
    // False for a quote, which is a picture of something said elsewhere: the
    // words to take are the ones in the message it was quoted from.
    selectable: bool,
) -> crate::ui::wash::Wash {
    let runs = runs(body, ranges, state, theme);
    // Shared rather than copied: the text is wanted twice, and a selectable run
    // is asked for it again on every frame it is on screen.
    let text = SharedString::from(runs.text);
    let washed = crate::ui::wash::wash(
        StyledText::new(text.clone())
            .with_highlights(runs.highlights)
            .with_font_family_overrides(runs.families),
        runs.boxed,
        theme.sunken,
        theme.border,
        INLINE_RADIUS,
        INLINE_PAD,
    )
    // Opaque, whatever the theme made of the token: a block you can read through
    // is not a spoiler.
    .covering(id.clone(), runs.covered, gpui::Hsla {
        a: 1.0,
        ..theme.text_muted
    });

    match selectable {
        true => washed.selectable(id, text, kit::selection(theme)),
        false => washed,
    }
}

/// What one paragraph comes to: the text actually drawn, and the three things
/// said about stretches of it.
struct Runs {
    text: String,
    highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    /// Which stretches are set in another family, sorted by where they start.
    families: Vec<(std::ops::Range<usize>, SharedString)>,
    /// Which stretches get a box painted behind them.
    boxed: Vec<std::ops::Range<usize>>,
    /// Which stretches get a block painted *over* them, until they are clicked.
    covered: Vec<std::ops::Range<usize>>,
}

fn runs(body: &str, ranges: &[Range], state: &State, theme: &Theme) -> Runs {
    let segments = format::segments(body, ranges);
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut mono = Vec::new();
    let mut serif = Vec::new();
    let mut boxed = Vec::new();
    let mut covered = Vec::new();

    for segment in segments {
        let styles = segment.styles;
        let start = text.len();

        match styles.mention {
            Some(uuid) => {
                text.push('@');
                text.push_str(&state.sender_name(uuid));
            }
            None if styles.monospace => {
                text.push_str(&body[segment.start..segment.end]);
            }
            // Maths is the one thing drawn as something other than what was
            // typed, so it is cut out of the run and put back as its symbols --
            // which means the highlight over the rest of the segment has to be
            // cut with it, or two overlapping ranges reach the layout.
            //
            // Only `$…$` reaches here. `$$…$$` was taken out of the paragraph by
            // `prose` and given an element of its own, because a display equation
            // is set larger and a text run carries no size: inline maths shares
            // the sentence's size and there is nothing to be done about that
            // without breaking the one layout a line needs to wrap as a line.
            None => {
                let source = &body[segment.start..segment.end];
                let mut upto = 0;

                for span in latex::spans(source) {
                    let plain = text.len();
                    text.push_str(&source[upto..span.start]);
                    note(&mut highlights, plain..text.len(), styles, theme);

                    let maths = text.len();
                    let read = latex::render(&span.tex);
                    text.push_str(&read);
                    note_maths(&mut highlights, maths, &read, styles, theme);
                    // The serif, for the same reason code gets the monospace: a
                    // family is not something `HighlightStyle` can set, and an
                    // integral sign in the interface font reads as a glyph
                    // somebody pasted rather than as an operator.
                    serif.push((maths..text.len(), theme.typography.serif.clone().into()));

                    upto = span.end;
                }

                let tail = text.len();
                text.push_str(&source[upto..]);
                note(&mut highlights, tail..text.len(), styles, theme);
            }
        }

        // `HighlightStyle` has no family to set, so a monospace *span* cannot be
        // spelled as a highlight -- which is why `` `bat` `` read as the body
        // font with a faint wash behind it. An override names the family for the
        // run, inside the one text layout, so the line still wraps as a line
        // rather than becoming a row of elements that wrap at their own edges.
        if styles.monospace {
            mono.push((start..text.len(), theme.typography.mono.clone().into()));
            boxed.push(start..text.len());
        }
        if styles.spoiler {
            covered.push(start..text.len());
        }
        // Everything but the maths branch styles the segment in one piece; that
        // one has already put its own pieces in.
        if styles.mention.is_some() {
            note(&mut highlights, start..text.len(), styles, theme);
        } else if styles.monospace {
            cut(&mut highlights, start..text.len(), styles, theme);
        }
    }

    // Sorted, because gpui requires it of the overrides and the two sources
    // arrive in an order of their own: a message with an equation before a word
    // in code hands over a serif range that starts after a monospace one and
    // sits before it, which the override pass then walks straight past.
    let mut families: Vec<_> = mono.into_iter().chain(serif).collect();
    families.sort_by_key(|(range, _)| range.start);

    Runs {
        text,
        highlights,
        families,
        boxed,
        covered,
    }
}

/// Makes a span a run of its own, whether or not it has anything to highlight.
///
/// A family override is applied to whole runs, and a run is only cut where a
/// highlight begins — so a span that highlights nothing is a span the override
/// slides straight past. That is why inline code was drawn in the body font: the
/// monospace family was named for a range no run ever lined up with. The style
/// may well be empty; being there is the whole job.
fn cut(
    highlights: &mut Vec<(std::ops::Range<usize>, HighlightStyle)>,
    range: std::ops::Range<usize>,
    styles: format::Styles,
    theme: &Theme,
) {
    if range.is_empty() {
        return;
    }
    highlights.push((range, highlight(styles, theme).unwrap_or_default()));
}

/// Records one piece's highlight, if it has one and covers anything at all.
fn note(
    highlights: &mut Vec<(std::ops::Range<usize>, HighlightStyle)>,
    range: std::ops::Range<usize>,
    styles: format::Styles,
    theme: &Theme,
) {
    if range.is_empty() {
        return;
    }
    if let Some(highlight) = highlight(styles, theme) {
        highlights.push((range, highlight));
    }
}

/// The same for a rendered equation, which is not one piece: the variables in it
/// are italic and everything else is upright, so it goes in as a run per stretch
/// of each. Setting the whole thing in italics -- which is what this did -- put
/// a slant on the digits, the brackets and the ∑, none of which is slanted in
/// any book, and made the equation read as an italicised sentence rather than as
/// maths. `latex::variables` is where the distinction is decided.
///
/// The ranges have to arrive in order and must not overlap, which is why the
/// upright stretches are emitted between the italic ones rather than as one
/// range underneath them.
fn note_maths(
    highlights: &mut Vec<(std::ops::Range<usize>, HighlightStyle)>,
    at: usize,
    rendered: &str,
    styles: format::Styles,
    theme: &Theme,
) {
    let base = highlight(styles, theme);
    let mut upto = 0;

    for variable in latex::variables(rendered) {
        // `cut` rather than `note`: the serif is a family override, and one only
        // reaches a run a highlight has cut. Left to `note`, the upright stretches
        // of an unstyled equation carried no highlight, so no run began at them
        // and the serif landed on the variables alone.
        cut(highlights, at + upto..at + variable.start, styles, theme);
        highlights.push((
            at + variable.start..at + variable.end,
            italicised(base.unwrap_or_default()),
        ));
        upto = variable.end;
    }

    cut(highlights, at + upto..at + rendered.len(), styles, theme);
}

/// A variable is set in italics, the way every typesetter sets one and the one
/// distinction available without a maths font.
fn italicised(mut highlight: HighlightStyle) -> HighlightStyle {
    highlight.font_style = Some(FontStyle::Italic);
    highlight
}

fn highlight(styles: format::Styles, theme: &Theme) -> Option<HighlightStyle> {
    let mut highlight = HighlightStyle::default();
    let mut touched = false;

    if styles.bold {
        highlight.font_weight = Some(kit::STRONG);
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
    // No background for monospace, and none for a spoiler: both are boxes
    // `ui::wash` paints, which is the only way to get the corners a highlight has
    // no room for.
    if styles.mention.is_some() {
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

/// Everything the three quote styles draw, resolved once so they cannot disagree
/// about what the message being answered actually said.
struct Quoted<'a> {
    author: String,
    /// What was said, styled as it was said. Absent for a picture with no
    /// caption, which has no words to quote.
    words: Option<crate::ui::wash::Wash>,
    /// What it was instead, when there are no words: "Photo", "Voice message".
    named: Option<String>,
    still: Option<std::path::PathBuf>,
    theme: &'a Theme,
    spacing: Spacing,
}

/// The message being answered, in whichever of the three shapes is asked for.
/// They differ only in how loud the context is allowed to be; all three carry the
/// same four things.
fn quoted(quote: &Quote, state: &State, theme: &Theme, spacing: Spacing, style: Reply) -> Div {
    // A picture with no caption has no text to quote, so the quote says what it
    // was instead of leaving a blank line where the words would have been. The
    // caption wins when there is one, because that is what was said.
    // Not selectable: a quote is a picture of something said elsewhere, and the
    // words to take are the ones in the message it was quoted from. A spoiler in
    // one is still a spoiler, and uncovering it here uncovers this copy of it:
    // the quote is named after the message it quotes, not after the one it is in.
    let words = (!quote.body.is_empty()).then(|| {
        styled(
            &quote.body,
            &quote.ranges,
            state,
            theme,
            SharedString::from(format!("quote-{}", quote.id.timestamp)),
            false,
        )
    });
    let named = words.is_none().then(|| quote.media.clone()).flatten();

    let quoted = Quoted {
        author: state.sender_name(quote.id.sender),
        words,
        named,
        still: match quote.thumbnail.as_ref().map(|thumbnail| &thumbnail.blob) {
            Some(Blob::Cached(path)) => Some(path.clone()),
            _ => None,
        },
        theme,
        spacing,
    };

    match style {
        Reply::Signal => quoted.signal(),
        Reply::Bar => quoted.bar(),
        Reply::Line => quoted.line(),
    }
}

impl Quoted<'_> {
    /// Square, at the height the two lines beside it come to, so a quote keeps
    /// its shape whether a still arrived or not.
    fn edge(&self) -> f32 {
        self.spacing.small * 2.0 + self.spacing.body
    }

    /// Who is being answered. Emphasised everywhere but the bar, which is the
    /// one shape whose whole point is to be quieter than what it sits above.
    ///
    /// In the text colour rather than in theirs. A name is already the answer to
    /// whose words these are; colouring it as well puts a second, brighter
    /// statement of the same thing inside every reply.
    fn who(&self, emphasis: bool) -> Div {
        div()
            .truncate()
            .text_size(px(self.spacing.small))
            .when(emphasis, |this| this.font_weight(kit::EMPHASIS))
            .text_color(self.theme.text_dim)
            .child(SharedString::from(self.author.clone()))
    }

    /// What was said, or what it was. One or the other: `named` only exists
    /// where there were no words.
    fn said(&mut self) -> Option<Div> {
        let line = div().truncate().text_size(px(self.spacing.small));

        match (self.words.take(), self.named.take()) {
            (Some(words), _) => Some(line.text_color(self.theme.text_muted).child(words)),
            (None, Some(named)) => Some(
                line.text_color(self.theme.text_dim)
                    .child(SharedString::from(named)),
            ),
            (None, None) => None,
        }
    }

    fn thumbnail(&mut self, edge: f32) -> Option<Div> {
        let path = self.still.take()?;

        Some(
            div()
                .flex_none()
                .size(px(edge))
                .overflow_hidden()
                .rounded(px(4.0))
                .bg(self.theme.surface)
                .child(image::cropped(path, edge)),
        )
    }

    /// Signal's own: a rounded block sat inside the message answering it, with
    /// the still at the far end.
    ///
    /// Filled in the theme's own quiet grey, and *not* in the sender's colour,
    /// which is the one thing about Signal's shape not worth copying: a hue
    /// generated per person puts a different bright rectangle in the middle of
    /// the thread for every person quoted, and a quote is the part of a reply
    /// nobody reads first. One fill, a step off whatever it sits on, and the name
    /// at the top of it is what says whose words these were. Rounded on every
    /// side, because what it is nested in is round too.
    fn signal(mut self) -> Div {
        let theme = self.theme;
        let edge = self.edge() + self.spacing.small;
        let said = self.said();
        let still = self.thumbnail(edge);
        let who = self.who(true).text_color(theme.text);

        div()
            .flex()
            .items_stretch()
            .overflow_hidden()
            .rounded(px(10.0))
            .bg(gpui::Hsla {
                a: 0.5,
                ..theme.sunken
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .gap_px()
                    .px_2p5()
                    .py_1p5()
                    .child(who)
                    .children(said),
            )
            .children(still)
    }

    /// A hairline with the quote beside it: context, kept as light as context can
    /// be drawn. What petunia had before there was a choice.
    fn bar(mut self) -> Div {
        let edge = self.edge();
        let said = self.said();
        let still = self.thumbnail(edge);
        let who = self.who(false);

        div()
            .flex()
            .gap_2p5()
            .child(div().w_px().flex_none().bg(self.theme.border_focus).rounded_full())
            .children(still)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap_px()
                    .child(who)
                    .children(said),
            )
    }

    /// One row: the reply mark, who, and what they said, truncated where the
    /// message is. Nothing is stacked, so a quote costs a line rather than a
    /// block.
    fn line(mut self) -> Div {
        let small = self.spacing.small;
        let said = self.said();
        let who = self.who(true);

        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(kit::icon(IconName::Undo, small, self.theme.text_muted)),
            )
            .child(who.flex_none())
            .children(said.map(|said| said.flex_1().min_w_0()))
    }
}

/// The card a sender attached to a link.
///
/// Rendered only for what arrives: fetching the page ourselves to build one
/// would tell a third party the moment a link reached this client, which is
/// exactly what the sender's own preview exists to avoid.
fn link_card(
    preview: &petunia_data::message::LinkPreview,
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

/// What an update says, in words. The duration is the whole of what anybody
/// wants from this line: "disappearing messages on" leaves the one question it
/// raises unanswered, and the answer is four characters long.
fn describe(update: &Update) -> String {
    match update {
        Update::ExpireTimer { seconds: 0 } => "Disappearing messages off".into(),
        Update::ExpireTimer { seconds } => format!(
            "Disappearing messages set to {}",
            crate::ui::details::timer_label(std::time::Duration::from_secs((*seconds).into()))
                .to_lowercase()
        ),
    }
}




#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> HighlightTheme {
        petunia_config::theme::dark().highlights()
    }

    fn drawn(input: &str) -> Runs {
        let (body, ranges) = markup::parse(input);
        let palette = petunia_config::theme::dark();
        runs(&body, &ranges, &State::new(uuid::Uuid::nil()), &palette)
    }

    /// gpui applies a family override to whole runs, and cuts a run only where a
    /// highlight begins. Inline code highlights nothing — it is a family and a box
    /// and no colour — so without a cut of its own the override lined up with no
    /// run and slid off, and `` `bat` `` came out in the body font.
    #[test]
    fn inline_code_is_a_run_of_its_own() {
        let runs = drawn("`bat` is a modern replacement for `cat`");

        assert_eq!(runs.text, "bat is a modern replacement for cat");
        assert_eq!(runs.families.len(), 2);
        for (range, _) in &runs.families {
            assert!(
                runs.highlights.iter().any(|(cut, _)| cut == range),
                "no run begins at {range:?}: {:?}",
                runs.highlights
            );
        }
        assert_eq!(runs.boxed, [0..3, 32..35]);
    }

    /// A spoiler is a block painted over the words, so the words are laid out as
    /// they were written: uncovering one is a repaint rather than a reflow, and
    /// nothing that holds an offset into the run — a selection, a highlight — has
    /// to be told. Replaced by blocks instead, as this once was, revealing a
    /// spoiler rewrote the paragraph under everything holding a position in it.
    #[test]
    fn a_spoiler_keeps_its_words_and_is_covered_where_they_are() {
        let runs = drawn("the butler ||did it|| really");

        assert_eq!(runs.text, "the butler did it really");
        assert_eq!(runs.covered, [11..17]);
    }

    /// Nothing else marks it: the cover is opaque, and a colour underneath one is
    /// a colour nobody can see until it is gone -- at which point the words want
    /// to look like the words around them.
    #[test]
    fn an_uncovered_spoiler_is_styled_like_anything_else() {
        let runs = drawn("||boo||");

        assert!(runs.highlights.is_empty(), "{:?}", runs.highlights);
    }

    /// The overrides have to arrive in order or the pass that applies them walks
    /// past one, and the two sources are not in order between themselves.
    #[test]
    fn family_overrides_are_sorted() {
        let runs = drawn("$x^2$ and then `code`");

        assert!(
            runs.families.windows(2).all(|pair| pair[0].0.start <= pair[1].0.start),
            "{:?}",
            runs.families.iter().map(|(range, _)| range.clone()).collect::<Vec<_>>()
        );
    }

    /// Display maths comes out of the paragraph so it can be set larger; inline
    /// maths stays in it, because a text run carries no size of its own.
    #[test]
    fn display_maths_is_a_block_and_inline_maths_is_not() {
        let body = "before $$x^2$$ after $y$ end";
        let found = blocks(body, &[]);

        assert_eq!(found.len(), 1);
        assert_eq!(&body[found[0].0..found[0].1], "$$x^2$$");
        assert!(matches!(&found[0].2, Block::Maths(tex) if tex == "x^2"));
    }

    /// A listing that happens to contain two dollars is a listing. The block wins,
    /// or the code would be cut in half by something inside it.
    #[test]
    fn maths_inside_a_code_block_stays_code() {
        let (body, ranges) = markup::parse("```\ncost = $$5\n```");
        let found = blocks(&body, &ranges);

        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].2, Block::Code));
    }

    /// Both kinds in one message, each getting its own element, in the order they
    /// were written.
    #[test]
    fn a_code_block_and_an_equation_are_both_blocks() {
        let (body, ranges) = markup::parse("```\nhi\n```\nand $$a+b$$\n");
        let mut found = blocks(&body, &ranges);
        found.sort_by_key(|(start, _, _)| *start);

        assert_eq!(found.len(), 2);
        assert!(matches!(found[0].2, Block::Code));
        assert!(matches!(found[1].2, Block::Maths(_)));
    }

    /// The point of the memo: the second ask is the same answer without the
    /// parse, because a visible code block is re-rendered every frame.
    #[test]
    fn highlighting_the_same_code_twice_parses_once() {
        let code = "key = \"value\"\n";
        let theme = theme();

        let first = memoized(code, "toml", &theme);
        let again = memoized(code, "toml", &theme);

        assert!(std::rc::Rc::ptr_eq(&first, &again));
        assert_eq!(*first, parse(code, "toml", &theme));
    }

    /// A theme change has to invalidate everything, or code keeps the colours of
    /// the theme it was first drawn under.
    #[test]
    fn a_new_theme_is_not_served_from_the_old_cache() {
        let code = "key = \"value\"\n";

        let dark = memoized(code, "toml", &theme());
        let light = memoized(code, "toml", &petunia_config::theme::light().highlights());

        assert!(!std::rc::Rc::ptr_eq(&dark, &light));
    }

    /// A language with no grammar is not an error: it comes back as one span, so
    /// the code is still drawn.
    #[test]
    fn an_unknown_language_still_yields_text() {
        let styles = memoized("hai", "nothing-like-this", &theme());

        assert!(styles.iter().all(|(range, _)| range.end <= 3));
    }

    /// Every range indexes the string that is handed to the renderer, which
    /// panics rather than truncates if one runs past the end or lands inside a
    /// character.
    #[test]
    fn every_range_is_a_valid_slice_of_the_code() {
        let code = "héllo = \"wörld\" # ünicode\n";

        for (range, _) in memoized(code, "toml", &theme()).iter() {
            assert!(code.is_char_boundary(range.start), "{range:?}");
            assert!(range.end <= code.len(), "{range:?}");
        }
    }
}
