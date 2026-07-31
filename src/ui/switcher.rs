//! Ranking for the quick switcher, kept apart from the view so the scoring
//! can be tested without one.

use crate::data::Index;
use crate::data::index::Entry;

/// Enough to fill the overlay without turning it into a list to read.
pub const LIMIT: usize = 12;

/// The entries a query selects, best first. An empty query is the recent
/// conversations, which is what the switcher is mostly used for.
pub fn matches<'a>(index: &'a Index, query: &str) -> Vec<&'a Entry> {
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

/// The row a step lands on, wrapping at both ends. Pure so the arithmetic can
/// be tested without a view; an empty list has nowhere to go.
pub fn step(selected: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as isize + delta).rem_euclid(len as isize) as usize
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
        assert_eq!(step(0, -1, 3), 2);
        assert_eq!(step(2, 1, 3), 0);
    }

    #[test]
    fn selection_is_safe_with_no_results() {
        assert_eq!(step(0, 1, 0), 0);
    }

    #[test]
    fn a_step_stays_inside_the_list() {
        for delta in [-9isize, -1, 0, 1, 9] {
            assert!(step(2, delta, 4) < 4);
        }
    }
}
