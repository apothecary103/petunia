use chrono::{DateTime, Local, NaiveDate};
use uuid::Uuid;

use crate::data::Message;
use crate::data::message::Content;

/// Bounds the widget tree per run, so one person monologuing does not build an
/// unbounded column.
const MAX_RUN: usize = 20;

#[derive(Debug)]
pub enum Entry<'a> {
    Day(NaiveDate),
    UnreadMarker,
    /// A consecutive stretch from one sender, rendered under a single header.
    Run(Run<'a>),
    /// Never grouped and never attributed: "disappearing messages on" is not
    /// something a person said.
    Update(&'a Message),
}

#[derive(Debug)]
pub struct Run<'a> {
    pub sender: Uuid,
    /// A subslice of the history, never a copy.
    pub messages: &'a [Message],
}

/// Splits a thread into day separators, runs and update lines. Pure so the
/// boundary conditions can be tested without a renderer.
pub fn entries<'a>(
    messages: &'a [Message],
    first_unread: Option<u64>,
    group_within_ms: u64,
) -> Vec<Entry<'a>> {
    let mut entries = Vec::new();
    let mut day: Option<NaiveDate> = None;
    let mut start = 0;

    while start < messages.len() {
        let message = &messages[start];
        let date = local_date(message.timestamp());

        if day != Some(date) {
            entries.push(Entry::Day(date));
            day = Some(date);
        }
        if first_unread == Some(message.timestamp()) {
            entries.push(Entry::UnreadMarker);
        }

        if matches!(message.content, Content::Update(_)) {
            entries.push(Entry::Update(message));
            start += 1;
            continue;
        }

        let end = run_end(messages, start, date, first_unread, group_within_ms);
        entries.push(Entry::Run(Run {
            sender: message.sender(),
            messages: &messages[start..end],
        }));
        start = end;
    }

    entries
}

/// The index one past the last message that belongs to the run starting at
/// `start`.
fn run_end(
    messages: &[Message],
    start: usize,
    date: NaiveDate,
    first_unread: Option<u64>,
    group_within_ms: u64,
) -> usize {
    let first = &messages[start];
    let mut end = start + 1;

    while end < messages.len() && end - start < MAX_RUN {
        let next = &messages[end];
        let breaks = next.sender() != first.sender()
            || matches!(next.content, Content::Update(_))
            || local_date(next.timestamp()) != date
            || first_unread == Some(next.timestamp())
            || next.timestamp().saturating_sub(messages[end - 1].timestamp()) > group_within_ms;

        if breaks {
            break;
        }
        end += 1;
    }
    end
}

pub fn local_date(timestamp: u64) -> NaiveDate {
    DateTime::from_timestamp_millis(timestamp as i64)
        .unwrap_or_default()
        .with_timezone(&Local)
        .date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::MessageId;

    /// A fixed, timezone-independent day boundary: these are milliseconds since
    /// the epoch, and every case stays inside one local day unless it says
    /// otherwise.
    const NOON: u64 = 1_700_000_000_000;
    const DAY: u64 = 24 * 60 * 60 * 1000;
    /// The default grouping window, five minutes.
    const GROUP_WITHIN: u64 = 5 * 60 * 1000;

    fn message(sender: Uuid, timestamp: u64) -> Message {
        Message::plain(MessageId { timestamp, sender }, "hi".into())
    }

    fn update(sender: Uuid, timestamp: u64) -> Message {
        let mut message = message(sender, timestamp);
        message.content = Content::Update(crate::data::message::Update::ExpireTimer {
            seconds: 60,
        });
        message
    }

    fn runs<'a>(entries: &'a [Entry<'a>]) -> Vec<&'a Run<'a>> {
        entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Run(run) => Some(run),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn one_sender_in_quick_succession_is_a_single_run() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            message(alice, NOON + 1000),
            message(alice, NOON + 2000),
        ];

        let entries = entries(&messages, None, GROUP_WITHIN);

        assert_eq!(runs(&entries).len(), 1);
        assert_eq!(runs(&entries)[0].messages.len(), 3);
    }

    #[test]
    fn a_different_sender_breaks_the_run() {
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let messages = [
            message(alice, NOON),
            message(bob, NOON + 1000),
            message(alice, NOON + 2000),
        ];

        let entries = entries(&messages, None, GROUP_WITHIN);
        let runs = runs(&entries);

        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].sender, alice);
        assert_eq!(runs[1].sender, bob);
    }

    #[test]
    fn a_gap_longer_than_the_window_breaks_the_run() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            message(alice, NOON + GROUP_WITHIN + 1),
        ];

        assert_eq!(runs(&entries(&messages, None, GROUP_WITHIN)).len(), 2);
    }

    #[test]
    fn a_gap_exactly_at_the_window_still_groups() {
        let alice = Uuid::new_v4();
        let messages = [message(alice, NOON), message(alice, NOON + GROUP_WITHIN)];

        assert_eq!(runs(&entries(&messages, None, GROUP_WITHIN)).len(), 1);
    }

    #[test]
    fn each_day_gets_one_separator_before_its_first_message() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            message(alice, NOON + 1000),
            message(alice, NOON + DAY),
        ];

        let entries = entries(&messages, None, GROUP_WITHIN);
        let days: Vec<_> = entries
            .iter()
            .filter(|entry| matches!(entry, Entry::Day(_)))
            .collect();

        assert_eq!(days.len(), 2);
        assert!(matches!(entries[0], Entry::Day(_)));
    }

    /// Derived through chrono rather than by hand, so the test holds in any
    /// timezone -- including the half-hour ones.
    fn next_local_midnight(after: u64) -> u64 {
        use chrono::TimeZone;

        let tomorrow = local_date(after).succ_opt().expect("a next day");
        Local
            .from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).expect("midnight"))
            .earliest()
            .expect("an unambiguous midnight")
            .timestamp_millis() as u64
    }

    #[test]
    fn a_day_change_breaks_the_run_even_within_the_window() {
        let alice = Uuid::new_v4();
        // Just before and just after local midnight, seconds apart.
        let midnight = next_local_midnight(NOON);
        let messages = [
            message(alice, midnight - 1000),
            message(alice, midnight + 1000),
        ];

        let entries = entries(&messages, None, GROUP_WITHIN);

        assert_eq!(runs(&entries).len(), 2);
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(entry, Entry::Day(_)))
                .count(),
            2
        );
    }

    #[test]
    fn an_update_is_never_grouped() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            update(alice, NOON + 1000),
            message(alice, NOON + 2000),
        ];

        let entries = entries(&messages, None, GROUP_WITHIN);

        assert_eq!(runs(&entries).len(), 2);
        assert!(
            entries
                .iter()
                .any(|entry| matches!(entry, Entry::Update(_)))
        );
    }

    #[test]
    fn the_unread_marker_lands_before_its_message_and_breaks_the_run() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            message(alice, NOON + 1000),
            message(alice, NOON + 2000),
        ];

        let entries = entries(&messages, Some(NOON + 1000), GROUP_WITHIN);

        let marker = entries
            .iter()
            .position(|entry| matches!(entry, Entry::UnreadMarker))
            .expect("marker present");
        assert!(matches!(entries[marker + 1], Entry::Run(_)));
        assert_eq!(runs(&entries).len(), 2);
        assert_eq!(runs(&entries)[1].messages[0].timestamp(), NOON + 1000);
    }

    #[test]
    fn an_unread_marker_for_the_first_message_comes_after_the_day_separator() {
        let alice = Uuid::new_v4();
        let messages = [message(alice, NOON)];

        let entries = entries(&messages, Some(NOON), GROUP_WITHIN);

        assert!(matches!(entries[0], Entry::Day(_)));
        assert!(matches!(entries[1], Entry::UnreadMarker));
    }

    #[test]
    fn a_long_monologue_is_split_at_the_run_cap() {
        let alice = Uuid::new_v4();
        let messages: Vec<_> = (0..MAX_RUN + 5)
            .map(|index| message(alice, NOON + index as u64 * 1000))
            .collect();

        let entries = entries(&messages, None, GROUP_WITHIN);
        let runs = runs(&entries);

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].messages.len(), MAX_RUN);
        assert_eq!(runs[1].messages.len(), 5);
    }

    #[test]
    fn an_empty_thread_has_no_entries() {
        assert!(entries(&[], None, GROUP_WITHIN).is_empty());
    }

    #[test]
    fn an_unread_marker_that_matches_nothing_is_not_shown() {
        let alice = Uuid::new_v4();
        let messages = [message(alice, NOON)];

        let entries = entries(&messages, Some(NOON + 99), GROUP_WITHIN);

        assert!(!entries.iter().any(|e| matches!(e, Entry::UnreadMarker)));
    }
}
