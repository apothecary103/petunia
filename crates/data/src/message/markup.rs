//! Markdown, in both directions.
//!
//! Signal carries formatting as `BodyRange` offsets over the body, not as
//! markup, and its own clients offer five styles and no way to type them. So
//! this does two jobs with one parser:
//!
//! - what you write is parsed on send, and the markers are stripped, so `*hai*`
//!   goes out as the word and a range that says it is italic. Every other client
//!   shows it italic, because that is what the protocol means.
//! - what arrives with *no* ranges is parsed for display, so a message someone
//!   typed as `*hai*` in another client reads as italic here rather than as
//!   three literal characters. A message that does carry ranges is left alone:
//!   the sender said what they meant, and a second pass over the same text would
//!   fight them.
//!
//! Anything markdown can express that a `BodyRange` cannot — headings, lists,
//! links with a label — is left as the characters that were typed. Sending a
//! heading that only petunia can see would be a lie about what the recipient
//! gets.

use super::{Range, range::Style};

/// The marker pairs, longest first so ``` is never read as a backtick and `**`
/// is never read as two `*`.
const MARKS: [(&str, Style); 9] = [
    ("```", Style::Monospace),
    ("||", Style::Spoiler),
    ("**", Style::Bold),
    ("__", Style::Bold),
    ("~~", Style::Strikethrough),
    ("*", Style::Italic),
    ("_", Style::Italic),
    ("~", Style::Strikethrough),
    ("`", Style::Monospace),
];

/// Markers made of underscores only count at a word boundary, or every
/// `snake_case_name` would come out half italic.
const WORDY: [&str; 2] = ["__", "_"];

/// Strips the markup and reports where the styles landed, in byte offsets over
/// the text that is left.
pub fn parse(input: &str) -> (String, Vec<Range>) {
    let mut body = String::with_capacity(input.len());
    let mut ranges = Vec::new();
    let mut open: Vec<(&str, Style, usize)> = Vec::new();
    let mut at = 0;

    while at < input.len() {
        let rest = &input[at..];

        // A backslash escapes the marker after it, so `\*` is a literal star.
        if let Some(escaped) = rest.strip_prefix('\\')
            && let Some(&(marker, _)) = marker_at(escaped)
        {
            body.push_str(marker);
            at += 1 + marker.len();
            continue;
        }

        // A fence is taken whole. Everything inside it is what was typed --
        // backticks, stars and all -- because that is what a code block means,
        // and scanning it for markers would eat the code.
        if rest.starts_with(FENCE) {
            at += fence(rest, &mut body, &mut ranges);
            continue;
        }

        match marker_at(rest).filter(|(marker, _)| usable(marker, input, at)) {
            Some(&(marker, style)) => {
                match open.iter().rposition(|(pending, _, _)| *pending == marker) {
                    // A closing marker with nothing between it and its opener is
                    // literal text: `**` alone is two stars, not empty bold.
                    Some(index) if open[index].2 < body.len() => {
                        let (_, style, start) = open.remove(index);
                        ranges.push(Range {
                            start,
                            len: body.len() - start,
                            style,
                        });
                    }
                    Some(index) => {
                        open.remove(index);
                        body.push_str(marker);
                        body.push_str(marker);
                    }
                    None => open.push((marker, style, body.len())),
                }
                at += marker.len();
            }
            None => {
                let character = rest.chars().next().expect("rest is non-empty");
                body.push(character);
                at += character.len_utf8();
            }
        }
    }

    // An opener that was never closed is just text the user typed. Putting it
    // back moves everything after it along, and *widens* anything it lands
    // inside: `**a*b**` is bold over three characters once the stray star is
    // text again, not over the two it covered while the star was a marker.
    for (marker, _, start) in open {
        body.insert_str(start, marker);
        for range in &mut ranges {
            if range.start >= start {
                range.start += marker.len();
            } else if range.end() > start {
                range.len += marker.len();
            }
        }
    }

    ranges.sort_by_key(|range| (range.start, range.len));
    (body, ranges)
}

const FENCE: &str = "```";

/// Takes a whole fenced block, returning how many input bytes it consumed.
///
/// Unlike every other marker, the fences are **kept**. Signal has one monospace
/// style and nowhere to put a language, so stripping them would throw the
/// language away — and a message of ours is re-read from the stored row, where
/// only the body and its ranges survive. Keeping them means the language is
/// still there when the block is drawn, and a client that has never heard of
/// code blocks shows a fenced listing in monospace, which is what it is.
///
/// An unclosed fence is left as the three characters that were typed, because
/// opening one that never closes would eat the rest of the message.
fn fence(rest: &str, body: &mut String, ranges: &mut Vec<Range>) -> usize {
    let opened = rest[FENCE.len()..]
        .find('\n')
        .map(|newline| FENCE.len() + newline + 1)
        .unwrap_or(rest.len());

    let Some(closes) = rest[opened..].find(FENCE) else {
        body.push_str(FENCE);
        return FENCE.len();
    };

    let taken = opened + closes + FENCE.len();
    let start = body.len();
    body.push_str(&rest[..taken]);
    ranges.push(Range {
        start,
        len: taken,
        style: Style::Monospace,
    });
    taken
}

/// Splits a fenced block into the language it declares and the code inside it.
/// `None` for anything that is not fenced, which is how inline code is told
/// apart from a block.
pub fn block(text: &str) -> Option<(Option<&str>, &str)> {
    let rest = text.strip_prefix(FENCE)?;
    let (info, body) = match rest.find('\n') {
        Some(newline) => (rest[..newline].trim(), &rest[newline + 1..]),
        None => (rest.trim(), ""),
    };

    let closed = body.strip_suffix(FENCE).unwrap_or(body);
    // The newline before the closing fence belongs to the fence, not to the
    // code, or every block ends with a blank line.
    let code = closed.strip_suffix('\n').unwrap_or(closed);

    Some((Some(info).filter(|info| !info.is_empty()), code))
}

/// The longest marker this text opens with.
fn marker_at(text: &str) -> Option<&'static (&'static str, Style)> {
    MARKS.iter().find(|(marker, _)| text.starts_with(marker))
}

/// Whether a marker at this offset means anything. Underscores inside a word
/// are part of the word.
fn usable(marker: &str, input: &str, at: usize) -> bool {
    if !WORDY.contains(&marker) {
        return true;
    }
    let before = input[..at].chars().next_back();
    let after = input[at + marker.len()..].chars().next();

    let boundary = |character: Option<char>| {
        character.is_none_or(|character| !character.is_alphanumeric())
    };
    boundary(before) || boundary(after)
}

/// Whether a monospace range is a code *block* rather than a word in code font.
/// The fences are still in the text, which is the whole difference.
pub fn is_block(body: &str, range: &Range) -> bool {
    if range.style != Style::Monospace || range.start > body.len() || range.end() > body.len() {
        return false;
    }
    block(&body[range.start..range.end()]).is_some()
}

/// Wraps the selected span in a marker, which is what a toolbar button does.
/// Returns the new text and where the selection should end up, so the caret does
/// not jump to the end of the message every time.
pub fn wrap(
    text: &str,
    selection: std::ops::Range<usize>,
    style: Style,
) -> (String, std::ops::Range<usize>) {
    let Some(marker) = marker_for(style) else {
        return (text.to_string(), selection);
    };
    let start = selection.start.min(text.len());
    let end = selection.end.clamp(start, text.len());

    let mut wrapped = String::with_capacity(text.len() + marker.len() * 2);
    wrapped.push_str(&text[..start]);
    wrapped.push_str(marker);
    wrapped.push_str(&text[start..end]);
    wrapped.push_str(marker);
    wrapped.push_str(&text[end..]);

    let moved = start + marker.len()..end + marker.len();
    (wrapped, moved)
}

/// The marker a button writes. The first spelling of each style wins, so bold is
/// `**` and italic is `*` — the underscore forms are read but never written.
fn marker_for(style: Style) -> Option<&'static str> {
    MARKS
        .iter()
        .filter(|(marker, _)| *marker != FENCE)
        .find(|(_, candidate)| *candidate == style)
        .map(|(marker, _)| *marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(input: &str) -> (String, Vec<(usize, usize, Style)>) {
        let (body, ranges) = parse(input);
        (
            body,
            ranges
                .into_iter()
                .map(|range| (range.start, range.len, range.style))
                .collect(),
        )
    }

    #[test]
    fn plain_text_is_left_alone() {
        assert_eq!(styles("hello there"), ("hello there".into(), vec![]));
    }

    #[test]
    fn each_marker_produces_its_style() {
        assert_eq!(styles("**b**").1, [(0, 1, Style::Bold)]);
        assert_eq!(styles("*i*").1, [(0, 1, Style::Italic)]);
        assert_eq!(styles("~s~").1, [(0, 1, Style::Strikethrough)]);
        assert_eq!(styles("`c`").1, [(0, 1, Style::Monospace)]);
        assert_eq!(styles("||x||").1, [(0, 1, Style::Spoiler)]);
    }

    /// The spelling everyone else's markdown uses, which is what people type.
    #[test]
    fn the_alternate_spellings_work_too() {
        assert_eq!(styles("__b__").1, [(0, 1, Style::Bold)]);
        assert_eq!(styles("_i_").1, [(0, 1, Style::Italic)]);
        assert_eq!(styles("~~s~~").1, [(0, 1, Style::Strikethrough)]);
    }

    /// What the user asked for, in as many words.
    #[test]
    fn a_starred_word_is_italic() {
        let (body, ranges) = styles("*hai*");

        assert_eq!(body, "hai");
        assert_eq!(ranges, [(0, 3, Style::Italic)]);
    }

    #[test]
    fn the_markers_are_removed_from_the_body() {
        assert_eq!(parse("say **this** now").0, "say this now");
    }

    /// The double marker has to win, or `**bold**` reads as an italic star.
    #[test]
    fn bold_is_not_read_as_two_italics() {
        assert_eq!(styles("**bold**"), ("bold".into(), vec![(0, 4, Style::Bold)]));
    }

    #[test]
    fn styles_nest() {
        let (body, ranges) = styles("**bold and *both* here**");

        assert_eq!(body, "bold and both here");
        assert_eq!(ranges, [(0, 18, Style::Bold), (9, 4, Style::Italic)]);
    }

    /// An unclosed marker is something the user typed, not a style, and losing
    /// it would silently eat a character out of their message.
    #[test]
    fn an_unclosed_marker_stays_as_text() {
        assert_eq!(styles("2 * 3 = 6"), ("2 * 3 = 6".into(), vec![]));
        assert_eq!(parse("**unfinished").0, "**unfinished");
    }

    /// Underscores inside a word are part of the word, which is the difference
    /// between a name and half of it in italics.
    #[test]
    fn an_underscore_inside_a_word_is_not_a_marker() {
        assert_eq!(styles("some_var_name"), ("some_var_name".into(), vec![]));
        assert_eq!(styles("read_to_string(path)").1, vec![]);
    }

    #[test]
    fn an_underscore_at_a_boundary_still_works() {
        assert_eq!(styles("say _this_ now").1, [(4, 4, Style::Italic)]);
    }

    /// The stray star goes back into the text, so the bold run has to grow with
    /// it. Otherwise the last character silently loses its style.
    #[test]
    fn a_style_widens_around_an_unclosed_marker_inside_it() {
        let (body, ranges) = styles("**a*b**");

        assert_eq!(body, "a*b");
        assert_eq!(ranges, [(0, 3, Style::Bold)]);
    }

    #[test]
    fn an_empty_pair_is_literal() {
        assert_eq!(styles("****"), ("****".into(), vec![]));
    }

    #[test]
    fn a_backslash_escapes_a_marker() {
        assert_eq!(styles(r"\*not italic\*"), ("*not italic*".into(), vec![]));
    }

    /// Offsets are bytes, and an emoji before a styled range is exactly where
    /// getting that wrong shows up.
    #[test]
    fn offsets_are_bytes_not_characters() {
        let (body, ranges) = styles("😀 **bold**");

        assert_eq!(body, "😀 bold");
        assert_eq!(ranges, [(5, 4, Style::Bold)]);
    }

    /// The fences stay, unlike every other marker: they are where the language
    /// lives, and the wire has nowhere else to put it.
    #[test]
    fn a_fenced_block_keeps_its_fences() {
        let (body, ranges) = styles("```\nlet x = 1;\n```");

        assert_eq!(body, "```\nlet x = 1;\n```");
        assert_eq!(ranges, [(0, 18, Style::Monospace)]);
    }

    #[test]
    fn a_block_yields_its_language_and_its_code() {
        assert_eq!(
            block("```rust\nlet x = 1;\n```"),
            Some((Some("rust"), "let x = 1;"))
        );
        assert_eq!(block("```\nplain\n```"), Some((None, "plain")));
        assert_eq!(block("not fenced"), None);
    }

    #[test]
    fn a_block_keeps_its_inner_newlines() {
        assert_eq!(block("```\none\ntwo\n```"), Some((None, "one\ntwo")));
    }

    #[test]
    fn text_around_a_fenced_block_survives() {
        let (body, ranges) = styles("before\n```\ncode\n```\nafter");

        assert_eq!(body, "before\n```\ncode\n```\nafter");
        assert_eq!(ranges, [(7, 12, Style::Monospace)]);
    }

    /// An unclosed fence would otherwise swallow the rest of the message.
    #[test]
    fn an_unclosed_fence_is_literal() {
        assert_eq!(parse("```\nnot closed").0, "```\nnot closed");
    }

    #[test]
    fn a_backtick_inside_a_fence_is_not_a_marker() {
        let (body, ranges) = parse("```\nlet x = `y`;\n```");

        assert_eq!(ranges.len(), 1);
        assert_eq!(block(&body).map(|(_, code)| code), Some("let x = `y`;"));
    }

    #[test]
    fn a_block_is_told_apart_from_inline_code() {
        let (body, ranges) = parse("```\none\ntwo\n```");
        assert!(is_block(&body, &ranges[0]));

        let (body, ranges) = parse("a `word` here");
        assert!(!is_block(&body, &ranges[0]));
    }

    #[test]
    fn wrapping_inserts_markers_around_the_selection() {
        let (text, selection) = wrap("hello world", 6..11, Style::Bold);

        assert_eq!(text, "hello **world**");
        assert_eq!(selection, 8..13);
        assert_eq!(parse(&text).0, "hello world");
    }

    /// Wrapping nothing gives you the markers with the caret between them,
    /// which is what you want when you press the button before typing.
    #[test]
    fn wrapping_an_empty_selection_gives_a_place_to_type() {
        let (text, selection) = wrap("hi ", 3..3, Style::Italic);

        assert_eq!(text, "hi **");
        assert_eq!(selection, 4..4);
    }

    #[test]
    fn wrapping_clamps_a_selection_past_the_end() {
        assert_eq!(wrap("hi", 0..99, Style::Monospace).0, "`hi`");
    }

    /// A button writes the starred spelling, never the underscored one, so what
    /// it inserts round-trips through the parser it was written for.
    #[test]
    fn buttons_write_a_spelling_the_parser_reads_back() {
        for style in [
            Style::Bold,
            Style::Italic,
            Style::Strikethrough,
            Style::Monospace,
            Style::Spoiler,
        ] {
            let (text, _) = wrap("word", 0..4, style);
            let (body, ranges) = parse(&text);

            assert_eq!(body, "word", "{style:?}");
            assert_eq!(ranges.len(), 1, "{style:?} produced {ranges:?}");
            assert_eq!(ranges[0].style, style);
        }
    }

    /// A mention has no spelling, so a button for one would have nothing to
    /// write.
    #[test]
    fn a_mention_cannot_be_typed() {
        let uuid = uuid::Uuid::new_v4();
        let (text, _) = wrap("word", 0..4, Style::Mention(uuid));

        assert_eq!(text, "word");
    }
}
