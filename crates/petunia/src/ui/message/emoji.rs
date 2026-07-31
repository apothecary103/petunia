//! Messages that are nothing but emoji, drawn at a size you can see.
//!
//! Signal enlarges a message made only of a few emoji: at body size a reaction
//! sent as a message reads as a typo. Past a handful they are a sentence made of
//! pictures rather than a gesture, and normal size is right again.

/// How much larger than the body an emoji-only message is drawn, or `None` if
/// this is ordinary text.
pub fn jumbo(body: &str) -> Option<f32> {
    let count = only_emoji(body)?;

    Some(match count {
        1 => 3.4,
        2 => 2.8,
        3 => 2.3,
        4 => 1.9,
        5 => 1.6,
        6 => 1.4,
        _ => return None,
    })
}

/// The number of emoji in a body that contains nothing else, or `None` the
/// moment anything else shows up.
fn only_emoji(body: &str) -> Option<usize> {
    let mut count = 0;
    let mut previous: Option<char> = None;

    for character in body.chars() {
        if character.is_whitespace() {
            previous = None;
            continue;
        }
        if joiner(character) {
            // A joiner with nothing before it is not an emoji on its own.
            previous?;
            previous = Some(character);
            continue;
        }
        if !pictographic(character) {
            return None;
        }
        // Anything after a zero-width joiner belongs to the cluster before it,
        // so a family of four draws once and counts once. A flag is a pair of
        // regional indicators for the same reason.
        let continues = previous == Some('\u{200D}')
            || (regional(character) && previous.is_some_and(regional));
        if !continues {
            count += 1;
        }
        previous = Some(character);
    }

    (count > 0).then_some(count)
}

fn regional(character: char) -> bool {
    matches!(character as u32, 0x1F1E6..=0x1F1FF)
}

/// Whether this character continues the cluster before it rather than starting
/// one: skin tones, variation selectors, keycaps and the zero-width joiner.
fn joiner(character: char) -> bool {
    matches!(
        character as u32,
        0x200D | 0xFE0E | 0xFE0F | 0x20E3 | 0x1F3FB..=0x1F3FF
    )
}

fn pictographic(character: char) -> bool {
    matches!(
        character as u32,
        0x00A9
            | 0x00AE
            | 0x203C
            | 0x2049
            | 0x2122
            | 0x2139
            | 0x3030
            | 0x303D
            | 0x3297
            | 0x3299
            | 0x2190..=0x21FF
            | 0x2300..=0x23FF
            | 0x25A0..=0x27BF
            | 0x2B00..=0x2BFF
            | 0x1F000..=0x1FAFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_emoji_is_the_largest() {
        assert_eq!(jumbo("😀"), Some(3.4));
    }

    #[test]
    fn more_emoji_means_smaller_ones() {
        let sizes: Vec<_> = ["😀", "😀😀", "😀😀😀", "😀😀😀😀"]
            .iter()
            .map(|body| jumbo(body).unwrap())
            .collect();

        assert!(sizes.windows(2).all(|pair| pair[0] > pair[1]));
    }

    /// Past a handful it is a sentence made of pictures, and a wall of 48px
    /// glyphs is unreadable.
    #[test]
    fn a_crowd_of_emoji_goes_back_to_body_size() {
        assert_eq!(jumbo("😀😀😀😀😀😀😀"), None);
    }

    #[test]
    fn any_text_at_all_disqualifies_it() {
        assert_eq!(jumbo("nice 😀"), None);
        assert_eq!(jumbo("😀!"), None);
        assert_eq!(jumbo(""), None);
        assert_eq!(jumbo("   "), None);
    }

    /// Space between emoji is still an emoji-only message.
    #[test]
    fn whitespace_between_emoji_is_allowed() {
        assert_eq!(jumbo("😀 🎉"), jumbo("😀🎉"));
    }

    /// A skin tone is a modifier, not a second emoji, or a waving hand would
    /// count twice and shrink.
    #[test]
    fn a_skin_tone_does_not_count_as_another_emoji() {
        assert_eq!(jumbo("👋🏽"), jumbo("👋"));
    }

    /// A zero-width-joined sequence draws as one glyph, so it counts as one.
    #[test]
    fn a_joined_sequence_counts_once() {
        assert_eq!(jumbo("👩‍💻"), jumbo("😀"));
    }

    #[test]
    fn a_variation_selector_does_not_count() {
        assert_eq!(jumbo("❤️"), jumbo("❤"));
    }

    /// A lone joiner is not an emoji and must not be read as the start of one.
    #[test]
    fn a_stray_joiner_is_not_an_emoji() {
        assert_eq!(jumbo("\u{200D}"), None);
        assert_eq!(jumbo("\u{FE0F}abc"), None);
    }
}
