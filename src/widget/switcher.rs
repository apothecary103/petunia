use iced::widget::{column, container, row, text, text_input};
use iced::{Center, Fill, Shrink, widget};

use super::{Element, avatar};
use crate::data::{Index, State, Thread};
use crate::theme;

/// How many results are worth showing before the list stops being a shortcut.
const LIMIT: usize = 12;

pub struct Switcher {
    query: String,
    selected: usize,
    input: widget::Id,
}

#[derive(Debug, Clone)]
pub enum Message {
    Query(String),
    Submit,
}

impl Switcher {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            input: widget::Id::unique(),
        }
    }

    pub fn input(&self) -> widget::Id {
        self.input.clone()
    }

    pub fn query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;
    }

    /// Wraps at both ends, so holding `down` cycles rather than sticking.
    pub fn move_by(&mut self, delta: isize, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }
        let total = total as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(total) as usize;
    }

    pub fn selection(&self, index: &Index) -> Option<Thread> {
        matches(index, &self.query)
            .get(self.selected)
            .map(|entry| entry.thread.clone())
    }

    pub fn count(&self, index: &Index) -> usize {
        matches(index, &self.query).len()
    }

    pub fn view<'a>(&'a self, state: &'a State) -> Element<'a, Message> {
        let results = matches(&state.index, &self.query);
        let colors = theme::colors();

        let rows = results.iter().enumerate().map(|(at, entry)| {
            let selected = at == self.selected;
            let accent = match &entry.thread {
                Thread::Contact(contact) => theme::accent(contact.uuid().as_bytes()),
                Thread::Group(key) => theme::accent(key),
            };

            container(
                row![
                    avatar::view(&entry.name, accent, 20.0, state.avatar(&entry.thread)),
                    text(&entry.name).size(13).height(Shrink).width(Fill),
                    text(if entry.unread > 0 {
                        entry.unread.to_string()
                    } else {
                        String::new()
                    })
                    .size(11)
                    .color(colors.danger)
                    .height(Shrink),
                ]
                .spacing(8)
                .align_y(Center),
            )
            .padding([4, 8])
            .width(Fill)
            .style(move |_theme| container::Style {
                background: selected.then_some(iced::Background::Color(iced::Color {
                    a: 0.22,
                    ..theme::colors().accent
                })),
                border: iced::border::rounded(5),
                ..container::Style::default()
            })
            .into()
        });

        let mut content = column![
            text_input("Jump to a conversation…", &self.query)
                .id(self.input.clone())
                .on_input(Message::Query)
                .on_submit(Message::Submit)
                .size(14)
                .padding([8, 10])
                .style(theme::message_input),
        ]
        .spacing(6);

        if results.is_empty() {
            content = content.push(
                container(text("No matches").size(12).style(theme::text_dim))
                    .padding([4, 8]),
            );
        } else {
            content = content.push(column(rows).spacing(1));
        }

        container(content)
            .padding(8)
            .width(460)
            .style(theme::overlay)
            .into()
    }
}

/// Ranked matches. An empty query lists the index as-is, which makes the
/// switcher a recent-conversations list before it is a search box.
fn matches<'a>(index: &'a Index, query: &str) -> Vec<&'a crate::data::index::Entry> {
    if query.trim().is_empty() {
        return index.conversations().take(LIMIT).collect();
    }

    let mut scored: Vec<_> = index
        .entries()
        .iter()
        .filter_map(|entry| score(&entry.name, query).map(|score| (score, entry)))
        .collect();

    // Higher score first, then by recency, which the index is already sorted by.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().take(LIMIT).map(|(_, entry)| entry).collect()
}

/// A subsequence match, scored so that tighter and earlier matches win. Hand
/// rolled: a fuzzy-match crate would be a dependency for forty lines.
pub fn score(name: &str, query: &str) -> Option<u32> {
    let haystack: Vec<char> = name.to_lowercase().chars().collect();
    let needle: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if needle.is_empty() {
        return Some(0);
    }

    let mut score = 0u32;
    let mut at = 0;
    let mut last: Option<usize> = None;

    for wanted in needle {
        let found = haystack[at..].iter().position(|c| *c == wanted)? + at;

        score += match last {
            // Adjacent characters are far more likely to be what was meant.
            Some(previous) if found == previous + 1 => 10,
            _ => 1,
        };
        // A match at a word boundary is a strong signal.
        if found == 0 || haystack.get(found - 1).is_some_and(|c| !c.is_alphanumeric()) {
            score += 6;
        }
        last = Some(found);
        at = found + 1;
    }

    // Prefer shorter names: "Bob" should beat "Bobby's Bakery Announcements".
    Some(score.saturating_add(20u32.saturating_sub(haystack.len().min(20) as u32)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_matches_everything() {
        assert!(score("Alice", "").is_some());
    }

    #[test]
    fn a_subsequence_matches_out_of_order_characters() {
        assert!(score("Alice Anderson", "aand").is_some());
        assert!(score("Alice", "ace").is_some());
    }

    #[test]
    fn a_missing_character_does_not_match() {
        assert!(score("Alice", "alz").is_none());
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(score("Alice", "ALICE").is_some());
    }

    #[test]
    fn a_contiguous_match_beats_a_scattered_one() {
        let contiguous = score("Alice", "ali").unwrap();
        let scattered = score("Aardvark Legion Ice", "ali").unwrap();

        assert!(contiguous > scattered, "{contiguous} vs {scattered}");
    }

    #[test]
    fn a_word_boundary_match_beats_a_mid_word_one() {
        let boundary = score("Bob Smith", "bs").unwrap();
        let midword = score("Bobsleigh", "bs").unwrap();

        assert!(boundary > midword, "{boundary} vs {midword}");
    }

    #[test]
    fn a_shorter_name_wins_when_the_match_is_otherwise_equal() {
        let short = score("Bob", "bob").unwrap();
        let long = score("Bobby's Bakery Announcements", "bob").unwrap();

        assert!(short > long, "{short} vs {long}");
    }

    #[test]
    fn whitespace_in_the_query_is_ignored() {
        assert_eq!(score("Alice", "a l i"), score("Alice", "ali"));
    }

    #[test]
    fn selection_wraps_at_both_ends() {
        let mut switcher = Switcher::new();

        switcher.move_by(-1, 3);
        assert_eq!(switcher.selected, 2);

        switcher.move_by(1, 3);
        assert_eq!(switcher.selected, 0);
    }

    #[test]
    fn selection_is_safe_with_no_results() {
        let mut switcher = Switcher::new();

        switcher.move_by(1, 0);
        assert_eq!(switcher.selected, 0);
    }

    #[test]
    fn typing_resets_the_selection() {
        let mut switcher = Switcher::new();
        switcher.move_by(2, 5);

        switcher.query("a".into());

        assert_eq!(switcher.selected, 0);
    }
}
