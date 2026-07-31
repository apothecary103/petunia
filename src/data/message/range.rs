use presage::libsignal_service::proto::BodyRange;
use presage::libsignal_service::proto::body_range::{AssociatedValue, Style as ProtoStyle};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub len: usize,
    pub style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Strikethrough,
    Monospace,
    Spoiler,
    Mention(Uuid),
}

impl Range {
    pub fn end(&self) -> usize {
        self.start + self.len
    }
}

/// Signal encodes body range offsets in UTF-16 code units; `Range` uses byte
/// offsets, because that is what `rich_text` slices. Mixing the two silently
/// misplaces formatting on any message containing a non-ASCII character.
pub fn from_proto(body: &str, ranges: &[BodyRange]) -> Vec<Range> {
    ranges
        .iter()
        .filter_map(|range| {
            let start = byte_offset(body, range.start() as usize);
            let end = byte_offset(body, (range.start() + range.length()) as usize);
            let style = style_from_proto(range.associated_value.as_ref()?)?;
            (end > start).then_some(Range {
                start,
                len: end - start,
                style,
            })
        })
        .collect()
}

pub fn to_proto(body: &str, ranges: &[Range]) -> Vec<BodyRange> {
    ranges
        .iter()
        .filter_map(|range| {
            let start = utf16_offset(body, range.start);
            let end = utf16_offset(body, range.end());
            (end > start).then(|| BodyRange {
                start: Some(start as u32),
                length: Some((end - start) as u32),
                associated_value: Some(style_to_proto(range.style)),
            })
        })
        .collect()
}

fn style_from_proto(value: &AssociatedValue) -> Option<Style> {
    match value {
        AssociatedValue::Style(style) => match ProtoStyle::try_from(*style).ok()? {
            ProtoStyle::Bold => Some(Style::Bold),
            ProtoStyle::Italic => Some(Style::Italic),
            ProtoStyle::Spoiler => Some(Style::Spoiler),
            ProtoStyle::Strikethrough => Some(Style::Strikethrough),
            ProtoStyle::Monospace => Some(Style::Monospace),
            ProtoStyle::None => None,
        },
        AssociatedValue::MentionAci(aci) => aci.parse().ok().map(Style::Mention),
        AssociatedValue::MentionAciBinary(bytes) => bytes
            .as_slice()
            .try_into()
            .ok()
            .map(|bytes| Style::Mention(Uuid::from_bytes(bytes))),
    }
}

fn style_to_proto(style: Style) -> AssociatedValue {
    match style {
        Style::Bold => AssociatedValue::Style(ProtoStyle::Bold as i32),
        Style::Italic => AssociatedValue::Style(ProtoStyle::Italic as i32),
        Style::Strikethrough => AssociatedValue::Style(ProtoStyle::Strikethrough as i32),
        Style::Monospace => AssociatedValue::Style(ProtoStyle::Monospace as i32),
        Style::Spoiler => AssociatedValue::Style(ProtoStyle::Spoiler as i32),
        Style::Mention(uuid) => AssociatedValue::MentionAciBinary(uuid.as_bytes().to_vec()),
    }
}

fn byte_offset(body: &str, target: usize) -> usize {
    let mut units = 0;
    for (byte, character) in body.char_indices() {
        if units >= target {
            return byte;
        }
        let next = units + character.len_utf16();
        if next > target {
            return byte;
        }
        units = next;
    }
    body.len()
}

fn utf16_offset(body: &str, target: usize) -> usize {
    let mut units = 0;
    for (byte, character) in body.char_indices() {
        if byte >= target {
            return units;
        }
        units += character.len_utf16();
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold(start: u32, length: u32) -> BodyRange {
        BodyRange {
            start: Some(start),
            length: Some(length),
            associated_value: Some(AssociatedValue::Style(ProtoStyle::Bold as i32)),
        }
    }

    fn styled(body: &str, ranges: &[Range]) -> Vec<String> {
        ranges
            .iter()
            .map(|range| body[range.start..range.end()].to_string())
            .collect()
    }

    #[test]
    fn maps_ascii_offsets_unchanged() {
        let body = "hello world";
        let ranges = from_proto(body, &[bold(6, 5)]);

        assert_eq!(ranges[0].start, 6);
        assert_eq!(ranges[0].len, 5);
        assert_eq!(styled(body, &ranges), ["world"]);
    }

    #[test]
    fn shifts_offsets_past_an_emoji() {
        // "😀" is one char, 4 bytes, but TWO UTF-16 code units.
        let body = "😀 bold";
        let ranges = from_proto(body, &[bold(3, 4)]);

        assert_eq!(styled(body, &ranges), ["bold"]);
    }

    #[test]
    fn shifts_offsets_past_multiple_emoji() {
        let body = "😀😀x";
        let ranges = from_proto(body, &[bold(4, 1)]);

        assert_eq!(styled(body, &ranges), ["x"]);
    }

    #[test]
    fn shifts_offsets_past_cjk() {
        // CJK is 3 bytes but only ONE UTF-16 code unit, so bytes run ahead here.
        let body = "日本語text";
        let ranges = from_proto(body, &[bold(3, 4)]);

        assert_eq!(ranges[0].start, 9);
        assert_eq!(styled(body, &ranges), ["text"]);
    }

    #[test]
    fn maps_a_range_covering_an_emoji() {
        let body = "a😀b";
        let ranges = from_proto(body, &[bold(1, 2)]);

        assert_eq!(styled(body, &ranges), ["😀"]);
    }

    #[test]
    fn round_trips_through_utf16_with_emoji() {
        let body = "😀 bold and 日本語";
        let original = from_proto(body, &[bold(3, 4)]);
        let round_tripped = from_proto(body, &to_proto(body, &original));

        assert_eq!(original, round_tripped);
        assert_eq!(styled(body, &round_tripped), ["bold"]);
    }

    #[test]
    fn round_trips_a_mention() {
        let uuid = Uuid::new_v4();
        let body = "hey 😀 you";
        let ranges = [Range {
            start: 0,
            len: 3,
            style: Style::Mention(uuid),
        }];

        let round_tripped = from_proto(body, &to_proto(body, &ranges));
        assert_eq!(round_tripped[0].style, Style::Mention(uuid));
        assert_eq!(styled(body, &round_tripped), ["hey"]);
    }

    #[test]
    fn clamps_ranges_past_the_end_of_the_body() {
        let body = "short";
        let ranges = from_proto(body, &[bold(2, 999)]);

        assert_eq!(styled(body, &ranges), ["ort"]);
    }

    #[test]
    fn drops_empty_and_styleless_ranges() {
        let body = "text";
        let none = BodyRange {
            start: Some(0),
            length: Some(4),
            associated_value: Some(AssociatedValue::Style(ProtoStyle::None as i32)),
        };

        assert!(from_proto(body, &[bold(0, 0)]).is_empty());
        assert!(from_proto(body, &[none]).is_empty());
        assert!(from_proto(body, &[bold(99, 4)]).is_empty());
    }

    #[test]
    fn reads_a_mention_from_either_proto_form() {
        let uuid = Uuid::new_v4();
        let text = BodyRange {
            start: Some(0),
            length: Some(1),
            associated_value: Some(AssociatedValue::MentionAci(uuid.to_string())),
        };
        let binary = BodyRange {
            start: Some(0),
            length: Some(1),
            associated_value: Some(AssociatedValue::MentionAciBinary(
                uuid.as_bytes().to_vec(),
            )),
        };

        assert_eq!(from_proto("x", &[text])[0].style, Style::Mention(uuid));
        assert_eq!(from_proto("x", &[binary])[0].style, Style::Mention(uuid));
    }
}
