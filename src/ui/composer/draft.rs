//! What is typed, turned into what goes on the wire.
//!
//! Signal carries formatting as `BodyRange` offsets, not as markup, so the
//! markers a person types have to be stripped and turned into ranges before the
//! message is sent. Doing it here, once, as a pure function keeps the composer
//! free of offset arithmetic and lets every boundary case be a test.

use crate::data::message::{Range, range::Style};

/// The marker pairs recognised, longest first so `||` is never read as two
/// separate `|`.
const MARKS: [(&str, Style); 5] = [
    ("||", Style::Spoiler),
    ("**", Style::Bold),
    ("*", Style::Italic),
    ("~", Style::Strikethrough),
    ("`", Style::Monospace),
];

/// Strips the markers and reports where the styles landed, in byte offsets over
/// the text that is left.
pub fn shortcuts(input: &str) -> (String, Vec<Range>) {
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

        match marker_at(rest) {
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

    // An opener that was never closed is just text the user typed.
    for (marker, _, start) in open {
        body.insert_str(start, marker);
        for range in &mut ranges {
            if range.start >= start {
                range.start += marker.len();
            }
        }
    }

    ranges.sort_by_key(|range| (range.start, range.len));
    (body, ranges)
}

/// Wraps the selected span in a marker, which is what a toolbar button does.
/// Returns the new text and where the selection should end up, so the caret does
/// not jump to the end of the message every time.
pub fn wrap(text: &str, selection: std::ops::Range<usize>, style: Style) -> (String, std::ops::Range<usize>) {
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

/// The longest marker this text opens with, so `||` is never read as two `|`.
fn marker_at(text: &str) -> Option<&'static (&'static str, Style)> {
    MARKS.iter().find(|(marker, _)| text.starts_with(marker))
}

fn marker_for(style: Style) -> Option<&'static str> {
    MARKS
        .iter()
        .find(|(_, candidate)| *candidate == style)
        .map(|(marker, _)| *marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(input: &str) -> (String, Vec<(usize, usize, Style)>) {
        let (body, ranges) = shortcuts(input);
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

    #[test]
    fn the_markers_are_removed_from_the_body() {
        assert_eq!(shortcuts("say **this** now").0, "say this now");
    }

    /// The double marker has to win, or `**bold**` reads as an italic star.
    #[test]
    fn bold_is_not_read_as_two_italics() {
        let (body, ranges) = styles("**bold**");

        assert_eq!(body, "bold");
        assert_eq!(ranges, [(0, 4, Style::Bold)]);
    }

    #[test]
    fn styles_nest() {
        let (body, ranges) = styles("**bold and *both* here**");

        assert_eq!(body, "bold and both here");
        assert_eq!(
            ranges,
            [(0, 18, Style::Bold), (9, 4, Style::Italic)]
        );
    }

    /// An unclosed marker is something the user typed, not a style, and losing
    /// it would silently eat a character out of their message.
    #[test]
    fn an_unclosed_marker_stays_as_text() {
        assert_eq!(styles("2 * 3 = 6"), ("2 * 3 = 6".into(), vec![]));
        assert_eq!(shortcuts("**unfinished").0, "**unfinished");
    }

    #[test]
    fn an_unclosed_marker_does_not_shift_a_real_range() {
        let (body, ranges) = styles("`code` and * a star");

        assert_eq!(body, "code and * a star");
        assert_eq!(ranges, [(0, 4, Style::Monospace)]);
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

    #[test]
    fn wrapping_inserts_markers_around_the_selection() {
        let (text, selection) = wrap("hello world", 6..11, Style::Bold);

        assert_eq!(text, "hello **world**");
        assert_eq!(selection, 8..13);
        assert_eq!(shortcuts(&text).0, "hello world");
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
        let (text, _) = wrap("hi", 0..99, Style::Monospace);

        assert_eq!(text, "`hi`");
    }
}
