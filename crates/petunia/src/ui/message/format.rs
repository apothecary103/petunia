use uuid::Uuid;

use petunia_data::message::{Range, range::Style};

/// Which styles apply to one stretch of bytes. Signal's ranges may overlap and
/// nest, so they are flattened to non-overlapping segments before rendering.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Styles {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub monospace: bool,
    pub spoiler: bool,
    pub mention: Option<Uuid>,
    pub link: bool,
}

impl Styles {
    fn add(&mut self, style: Style) {
        match style {
            Style::Bold => self.bold = true,
            Style::Italic => self.italic = true,
            Style::Strikethrough => self.strikethrough = true,
            Style::Monospace => self.monospace = true,
            Style::Spoiler => self.spoiler = true,
            Style::Mention(uuid) => self.mention = Some(uuid),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub styles: Styles,
}

/// Cuts the body at every range boundary, so each segment has one flat style set.
pub fn segments(body: &str, ranges: &[Range]) -> Vec<Segment> {
    let links = urls(body);
    let mut cuts = vec![0, body.len()];
    for range in ranges {
        cuts.push(range.start.min(body.len()));
        cuts.push(range.end().min(body.len()));
    }
    for (start, end) in &links {
        cuts.push(*start);
        cuts.push(*end);
    }
    cuts.retain(|cut| body.is_char_boundary(*cut));
    cuts.sort_unstable();
    cuts.dedup();

    cuts.windows(2)
        .filter(|window| window[1] > window[0])
        .map(|window| {
            let (start, end) = (window[0], window[1]);
            let mut styles = Styles::default();
            for range in ranges {
                if range.start <= start && end <= range.end() {
                    styles.add(range.style);
                }
            }
            styles.link = links
                .iter()
                .any(|(from, to)| *from <= start && end <= *to);
            Segment { start, end, styles }
        })
        .collect()
}

/// Bare URLs in the body, so they can be styled and opened without the sender
/// having marked them up.
pub fn urls(body: &str) -> Vec<(usize, usize)> {
    const SCHEMES: [&str; 2] = ["https://", "http://"];
    let mut found = Vec::new();
    let mut cursor = 0;

    while cursor < body.len() {
        let rest = &body[cursor..];
        let Some((offset, scheme)) = SCHEMES
            .iter()
            .filter_map(|scheme| rest.find(scheme).map(|at| (at, *scheme)))
            .min_by_key(|(at, _)| *at)
        else {
            break;
        };

        let start = cursor + offset;
        let after = start + scheme.len();
        let end = body[after..]
            .find(|c: char| c.is_whitespace())
            .map_or(body.len(), |at| after + at);
        // Trailing punctuation almost always belongs to the sentence, not the URL.
        let end = body[..end].trim_end_matches(['.', ',', ')', ']', '!', '?', ';', ':']).len();

        if end > after {
            found.push((start, end));
        }
        cursor = end.max(after);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(body: &str) -> Vec<Segment> {
        segments(body, &[])
    }

    #[test]
    fn an_unstyled_body_is_one_segment() {
        let segments = plain("hello there");

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].styles, Styles::default());
    }

    #[test]
    fn a_range_cuts_the_body_into_three() {
        let ranges = [Range {
            start: 6,
            len: 5,
            style: Style::Bold,
        }];
        let segments = segments("hello world again", &ranges);

        assert_eq!(segments.len(), 3);
        assert!(!segments[0].styles.bold);
        assert!(segments[1].styles.bold);
        assert_eq!((segments[1].start, segments[1].end), (6, 11));
        assert!(!segments[2].styles.bold);
    }

    #[test]
    fn overlapping_ranges_flatten_into_a_combined_segment() {
        let ranges = [
            Range {
                start: 0,
                len: 8,
                style: Style::Bold,
            },
            Range {
                start: 4,
                len: 8,
                style: Style::Italic,
            },
        ];
        let segments = segments("aaaabbbbcccc", &ranges);

        assert_eq!(segments.len(), 3);
        assert!(segments[0].styles.bold && !segments[0].styles.italic);
        assert!(segments[1].styles.bold && segments[1].styles.italic);
        assert!(!segments[2].styles.bold && segments[2].styles.italic);
    }

    /// The UTF-16 conversion happens at the protocol boundary, so by the time a
    /// Range reaches here it is byte-based -- but a cut must still never land
    /// inside a character.
    #[test]
    fn segments_never_split_a_multibyte_character() {
        let body = "héllo 🎉 world";
        let ranges = [Range {
            start: 1,
            len: 2,
            style: Style::Bold,
        }];

        for segment in segments(body, &ranges) {
            assert!(body.is_char_boundary(segment.start));
            assert!(body.is_char_boundary(segment.end));
            let _ = &body[segment.start..segment.end];
        }
    }

    #[test]
    fn a_range_past_the_end_is_clamped() {
        let ranges = [Range {
            start: 2,
            len: 999,
            style: Style::Bold,
        }];

        let segments = segments("abcd", &ranges);

        assert_eq!(segments.last().unwrap().end, 4);
    }

    #[test]
    fn finds_a_bare_url() {
        assert_eq!(urls("see https://example.com ok"), [(4, 23)]);
    }

    #[test]
    fn drops_trailing_sentence_punctuation() {
        let body = "go to https://example.com.";
        let (start, end) = urls(body)[0];

        assert_eq!(&body[start..end], "https://example.com");
    }

    #[test]
    fn finds_several_urls() {
        assert_eq!(urls("http://a.com and https://b.com").len(), 2);
    }

    #[test]
    fn ignores_text_without_a_scheme() {
        assert!(urls("example.com is not linkified").is_empty());
    }

    #[test]
    fn a_url_at_the_very_end_is_found() {
        let body = "visit https://example.com";
        let (start, end) = urls(body)[0];

        assert_eq!(&body[start..end], "https://example.com");
    }

    #[test]
    fn a_scheme_with_nothing_after_it_is_not_a_url() {
        assert!(urls("https://").is_empty());
    }

    #[test]
    fn a_url_becomes_its_own_segment() {
        let body = "a https://example.com b";
        let segments = segments(body, &[]);

        let link = segments
            .iter()
            .find(|segment| segment.styles.link)
            .expect("a link segment");
        assert_eq!(&body[link.start..link.end], "https://example.com");
    }
}
