use std::ops::Range;

use chrono::{DateTime, Local, NaiveDate};
use uuid::Uuid;

use crate::data::Message;
use crate::data::message::Content;

/// Bounds the widget tree per run, so one person monologuing does not build an
/// unbounded column.
const MAX_RUN: usize = 20;

/// One row of the message list. Positions rather than borrows: the list element
/// renders a row on demand, long after the frame that decided where the rows
/// were, so a row that held a `&[Message]` could not outlive the loop that
/// produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Day(NaiveDate),
    UnreadMarker,
    /// A consecutive stretch from one sender, rendered under a single header.
    Run { sender: Uuid, messages: Range<usize> },
    /// Never grouped and never attributed: "disappearing messages on" is not
    /// something a person said.
    Update(usize),
}

/// Splits a thread into day separators, runs and update lines. Pure so the
/// boundary conditions can be tested without a renderer.
pub fn rows(messages: &[Message], first_unread: Option<u64>, group_within_ms: u64) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut day: Option<NaiveDate> = None;
    let mut start = 0;

    while start < messages.len() {
        let message = &messages[start];
        let date = local_date(message.timestamp());

        if day != Some(date) {
            rows.push(Row::Day(date));
            day = Some(date);
        }
        if first_unread == Some(message.timestamp()) {
            rows.push(Row::UnreadMarker);
        }

        if matches!(message.content, Content::Update(_)) {
            rows.push(Row::Update(start));
            start += 1;
            continue;
        }

        let end = run_end(messages, start, date, first_unread, group_within_ms);
        rows.push(Row::Run {
            sender: message.sender(),
            messages: start..end,
        });
        start = end;
    }

    rows
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

    /// The runs, as (sender, how many messages) -- which is what every case here
    /// is actually about.
    fn runs(rows: &[Row]) -> Vec<(Uuid, usize)> {
        rows.iter()
            .filter_map(|row| match row {
                Row::Run { sender, messages } => Some((*sender, messages.len())),
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

        assert_eq!(runs(&rows(&messages, None, GROUP_WITHIN)), [(alice, 3)]);
    }

    #[test]
    fn a_different_sender_breaks_the_run() {
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let messages = [
            message(alice, NOON),
            message(bob, NOON + 1000),
            message(alice, NOON + 2000),
        ];

        assert_eq!(
            runs(&rows(&messages, None, GROUP_WITHIN)),
            [(alice, 1), (bob, 1), (alice, 1)]
        );
    }

    #[test]
    fn a_gap_longer_than_the_window_breaks_the_run() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            message(alice, NOON + GROUP_WITHIN + 1),
        ];

        assert_eq!(runs(&rows(&messages, None, GROUP_WITHIN)).len(), 2);
    }

    #[test]
    fn a_gap_exactly_at_the_window_still_groups() {
        let alice = Uuid::new_v4();
        let messages = [message(alice, NOON), message(alice, NOON + GROUP_WITHIN)];

        assert_eq!(runs(&rows(&messages, None, GROUP_WITHIN)).len(), 1);
    }

    #[test]
    fn each_day_gets_one_separator_before_its_first_message() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            message(alice, NOON + 1000),
            message(alice, NOON + DAY),
        ];

        let rows = rows(&messages, None, GROUP_WITHIN);
        let days = rows.iter().filter(|row| matches!(row, Row::Day(_))).count();

        assert_eq!(days, 2);
        assert!(matches!(rows[0], Row::Day(_)));
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

        let rows = rows(&messages, None, GROUP_WITHIN);

        assert_eq!(runs(&rows).len(), 2);
        assert_eq!(
            rows.iter().filter(|row| matches!(row, Row::Day(_))).count(),
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

        let rows = rows(&messages, None, GROUP_WITHIN);

        assert_eq!(runs(&rows).len(), 2);
        assert!(rows.contains(&Row::Update(1)));
    }

    #[test]
    fn the_unread_marker_lands_before_its_message_and_breaks_the_run() {
        let alice = Uuid::new_v4();
        let messages = [
            message(alice, NOON),
            message(alice, NOON + 1000),
            message(alice, NOON + 2000),
        ];

        let rows = rows(&messages, Some(NOON + 1000), GROUP_WITHIN);

        let marker = rows
            .iter()
            .position(|row| matches!(row, Row::UnreadMarker))
            .expect("marker present");

        // The run after the marker starts at the message the marker is for.
        assert_eq!(
            rows[marker + 1],
            Row::Run {
                sender: alice,
                messages: 1..3
            }
        );
        assert_eq!(runs(&rows).len(), 2);
    }

    #[test]
    fn an_unread_marker_for_the_first_message_comes_after_the_day_separator() {
        let alice = Uuid::new_v4();
        let messages = [message(alice, NOON)];

        let rows = rows(&messages, Some(NOON), GROUP_WITHIN);

        assert!(matches!(rows[0], Row::Day(_)));
        assert!(matches!(rows[1], Row::UnreadMarker));
    }

    #[test]
    fn a_long_monologue_is_split_at_the_run_cap() {
        let alice = Uuid::new_v4();
        let messages: Vec<_> = (0..MAX_RUN + 5)
            .map(|index| message(alice, NOON + index as u64 * 1000))
            .collect();

        assert_eq!(
            runs(&rows(&messages, None, GROUP_WITHIN)),
            [(alice, MAX_RUN), (alice, 5)]
        );
    }

    /// Every row addresses the history by position, so a range that runs off the
    /// end would panic in the renderer rather than here.
    #[test]
    fn every_row_points_inside_the_history() {
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let messages = [
            message(alice, NOON),
            update(alice, NOON + 1000),
            message(bob, NOON + 2000),
            message(bob, NOON + 3000),
        ];

        for row in rows(&messages, Some(NOON + 2000), GROUP_WITHIN) {
            match row {
                Row::Run { messages: range, .. } => {
                    assert!(range.start < range.end);
                    assert!(range.end <= messages.len(), "{range:?}");
                }
                Row::Update(index) => assert!(index < messages.len()),
                Row::Day(_) | Row::UnreadMarker => {}
            }
        }
    }

    #[test]
    fn an_empty_thread_has_no_rows() {
        assert!(rows(&[], None, GROUP_WITHIN).is_empty());
    }

    #[test]
    fn an_unread_marker_that_matches_nothing_is_not_shown() {
        let alice = Uuid::new_v4();
        let messages = [message(alice, NOON)];

        let rows = rows(&messages, Some(NOON + 99), GROUP_WITHIN);

        assert!(!rows.iter().any(|row| matches!(row, Row::UnreadMarker)));
    }
}
