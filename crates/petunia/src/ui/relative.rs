use chrono::{DateTime, Local};

/// The compact age of a timestamp: `5m`, `3h`, `2d`. Empty when there is no
/// time to show, so a row with nothing in it stays quiet rather than reading
/// "0m".
pub fn short(timestamp: u64) -> String {
    if timestamp == 0 {
        return String::new();
    }
    let Some(at) = local(timestamp) else {
        return String::new();
    };

    let seconds = (Local::now() - at).num_seconds().max(0);
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    match (minutes, hours, days) {
        (_, _, days) if days >= 365 => format!("{}y", days / 365),
        (_, _, days) if days >= 7 => format!("{}w", days / 7),
        (_, _, days) if days >= 1 => format!("{days}d"),
        (_, hours, _) if hours >= 1 => format!("{hours}h"),
        (minutes, _, _) if minutes >= 1 => format!("{minutes}m"),
        _ => "now".into(),
    }
}

pub fn local(timestamp: u64) -> Option<DateTime<Local>> {
    DateTime::from_timestamp_millis(timestamp as i64).map(|at| at.with_timezone(&Local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn ago(duration: Duration) -> u64 {
        (Local::now() - duration).timestamp_millis() as u64
    }

    #[test]
    fn scales_through_the_units() {
        assert_eq!(short(ago(Duration::seconds(5))), "now");
        assert_eq!(short(ago(Duration::minutes(5))), "5m");
        assert_eq!(short(ago(Duration::hours(3))), "3h");
        assert_eq!(short(ago(Duration::days(2))), "2d");
        assert_eq!(short(ago(Duration::days(21))), "3w");
        assert_eq!(short(ago(Duration::days(800))), "2y");
    }

    /// A thread nothing has happened in carries a zero timestamp, which must
    /// read as nothing rather than as half a century.
    #[test]
    fn an_absent_time_says_nothing() {
        assert_eq!(short(0), "");
    }

    /// Clock skew between devices can date a message slightly in the future.
    #[test]
    fn a_future_timestamp_does_not_go_negative() {
        let ahead = (Local::now() + Duration::minutes(5)).timestamp_millis() as u64;

        assert_eq!(short(ahead), "now");
    }
}
