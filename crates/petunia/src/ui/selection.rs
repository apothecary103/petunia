//! What is selected in a message, and copying it.
//!
//! gpui has no selectable text outside its own input: a message is a laid-out
//! run of glyphs and nothing else, so selecting some of it is petunia's own
//! work. One selection, in one run of text, kept in a global — because the thing
//! that draws the wash and the thing that hears the mouse are the same element,
//! rebuilt every frame, with nowhere of its own to keep anything. A view could
//! hold it instead, but every message in the conversation would then have to be
//! handed a callback to reach it, and a quote in a details panel could not have
//! one at all.
//!
//! Signal's behaviour, which is the browser's: a drag selects what it crosses, a
//! double click selects a word, a third click takes the paragraph, and `cmd-c`
//! copies whatever is lit. Within one run, not across several — a selection that
//! spans messages is a second thing entirely, and nothing here pretends to be
//! one.

use std::ops::Range;

use gpui::{App, Global, SharedString};

/// The one selection there is.
#[derive(Default)]
struct Selected(Option<Selection>);

impl Global for Selected {}

struct Selection {
    /// Which run of text it is in.
    of: SharedString,
    /// That run, whole, so copying needs nothing but this.
    text: SharedString,
    /// Where the drag started. Either end of the range, depending on which way
    /// it went.
    anchor: usize,
    head: usize,
    /// Whether the mouse is still down, which is what makes a move an extension
    /// rather than a hover.
    dragging: bool,
}

impl Selection {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// What is selected in this run, if the selection is in this run at all.
pub fn within(of: &SharedString, cx: &App) -> Option<Range<usize>> {
    let selected = cx.try_global::<Selected>()?.0.as_ref()?;
    (selected.of == *of && !selected.range().is_empty()).then(|| selected.range())
}

/// Puts the anchor down. Nothing is lit until the pointer moves, so a plain
/// click reads as what it is: a click that happened to land on some words.
pub fn begin(of: SharedString, text: SharedString, at: usize, cx: &mut App) {
    cx.default_global::<Selected>().0 = Some(Selection {
        of,
        text,
        anchor: at,
        head: at,
        dragging: true,
    });
}

/// Lights a range outright, for the clicks that mean a word or a paragraph.
pub fn lit(of: SharedString, text: SharedString, range: Range<usize>, cx: &mut App) {
    cx.default_global::<Selected>().0 = Some(Selection {
        of,
        text,
        anchor: range.start,
        head: range.end,
        dragging: false,
    });
}

/// Whether a drag that began in this run is still going.
pub fn dragging(of: &SharedString, cx: &App) -> bool {
    cx.try_global::<Selected>()
        .and_then(|selected| selected.0.as_ref())
        .is_some_and(|selected| selected.dragging && selected.of == *of)
}

pub fn extend(to: usize, cx: &mut App) {
    if let Some(selected) = cx.default_global::<Selected>().0.as_mut() {
        selected.head = to;
    }
}

pub fn release(cx: &mut App) {
    if let Some(selected) = cx.default_global::<Selected>().0.as_mut() {
        selected.dragging = false;
    }
}

pub fn clear(cx: &mut App) {
    cx.default_global::<Selected>().0 = None;
}

/// Copies what is lit, and answers whether there was anything.
pub fn copy(cx: &mut App) -> bool {
    // Nothing lit is nothing copied: a plain click leaves an anchor and an empty
    // range behind it, and emptying the clipboard is not what cmd-c means.
    let Some(text) = cx
        .try_global::<Selected>()
        .and_then(|selected| selected.0.as_ref())
        .filter(|selected| !selected.range().is_empty())
        .and_then(|selected| selected.text.get(selected.range()))
        .map(str::to_owned)
    else {
        return false;
    };
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    true
}

/// The word around a byte offset, as a double click means it: a run of letters,
/// digits and the marks that live inside a word, or the run of whitespace or
/// punctuation the pointer landed in instead.
pub fn word(text: &str, at: usize) -> Range<usize> {
    let at = floor(text, at.min(text.len()));
    let kind = |ch: char| match ch {
        ch if ch.is_alphanumeric() || ch == '_' || ch == '\'' => Kind::Word,
        ch if ch.is_whitespace() => Kind::Space,
        _ => Kind::Other,
    };
    // On a boundary the word *behind* the pointer wins, which is what a click in
    // the gap after a word means everywhere else: "hello| there" selects hello.
    let here = match (
        text[..at].chars().next_back().map(kind),
        text[at..].chars().next().map(kind),
    ) {
        (Some(Kind::Word), _) => Kind::Word,
        (_, Some(kind)) | (Some(kind), None) => kind,
        (None, None) => return 0..0,
    };

    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| kind(*ch) == here)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(at);
    let end = text[at..]
        .char_indices()
        .take_while(|(_, ch)| kind(*ch) == here)
        .map(|(index, ch)| at + index + ch.len_utf8())
        .last()
        .unwrap_or(at);

    start..end
}

#[derive(PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Other,
}

/// The nearest character boundary at or before an offset. `index_for_position`
/// answers in bytes off a laid-out line, and a line laid out from text with
/// anything multi-byte in it can name the middle of a character.
fn floor(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_the_letters_around_the_pointer() {
        assert_eq!(word("hello there", 3), 0..5);
        assert_eq!(word("hello there", 7), 6..11);
    }

    #[test]
    fn a_click_at_the_end_of_a_word_takes_that_word() {
        assert_eq!(word("hello there", 5), 0..5);
        assert_eq!(word("hello", 5), 0..5);
    }

    #[test]
    fn punctuation_is_not_part_of_a_word() {
        assert_eq!(word("stop. go", 2), 0..4);
        assert_eq!(word("(hi)", 1), 1..3);
    }

    #[test]
    fn an_apostrophe_is() {
        assert_eq!(word("don't stop", 2), 0..5);
    }

    #[test]
    fn a_click_in_the_gap_takes_the_gap() {
        assert_eq!(word("a   b", 2), 1..4);
    }

    #[test]
    fn nothing_selects_nothing() {
        assert_eq!(word("", 0), 0..0);
    }

    /// A byte offset from a laid-out line can land inside a character, and
    /// slicing there panics.
    #[test]
    fn an_offset_inside_a_character_is_taken_back_to_its_start() {
        let text = "héllo";
        assert_eq!(word(text, 2), 0..6);
    }

    #[test]
    fn an_offset_past_the_end_is_clamped() {
        assert_eq!(word("hi", 99), 0..2);
    }
}
