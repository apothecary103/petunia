use std::collections::HashMap;

use sqlx::{AssertSqlSafe, Row};
use uuid::Uuid;

use super::Db;
use crate::data::{Status, Thread};
use crate::signal::Error;

impl Db {
    /// Records the local send lifecycle for one of our own messages.
    pub async fn set_send_state(
        &self,
        timestamp: u64,
        thread: &Thread,
        status: Status,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_send (ts, thread, state) VALUES (?, ?, ?)
            ON CONFLICT DO UPDATE SET state = excluded.state, thread = excluded.thread",
        )
        .bind(timestamp as i64)
        .bind(thread_key(thread))
        .bind(to_int(status))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A `Sending` row left behind by a previous run means the outcome was never
    /// recorded, so the send did not complete. Swept once at startup rather than
    /// reinterpreted on read, which would misreport sends still in flight.
    pub async fn fail_stale_sends(&self) -> Result<u64, Error> {
        let result = sqlx::query("UPDATE petunia_send SET state = ? WHERE state = ?")
            .bind(to_int(Status::Failed))
            .bind(to_int(Status::Sending))
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn record_receipts(
        &self,
        timestamps: &[u64],
        recipient: Uuid,
        status: Status,
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        for &timestamp in timestamps {
            sqlx::query(
                "INSERT INTO petunia_receipt (ts, recipient, state) VALUES (?, ?, ?)
                ON CONFLICT DO UPDATE SET state = MAX(state, excluded.state)",
            )
            .bind(timestamp as i64)
            .bind(recipient.as_bytes().as_slice())
            .bind(to_int(status))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Resolves the status shown for each of our messages, combining the local
    /// send state with the per-recipient receipts collected so far.
    pub async fn statuses(
        &self,
        timestamps: &[u64],
        recipients: usize,
    ) -> Result<HashMap<u64, Status>, Error> {
        let sends = self.send_states(timestamps).await?;
        let receipts = self.receipts(timestamps).await?;

        Ok(timestamps
            .iter()
            .filter_map(|timestamp| {
                let send = sends.get(timestamp).copied();
                let received = receipts.get(timestamp).map(Vec::as_slice).unwrap_or(&[]);
                aggregate(send, received, recipients).map(|status| (*timestamp, status))
            })
            .collect())
    }

    async fn send_states(&self, timestamps: &[u64]) -> Result<HashMap<u64, Status>, Error> {
        let mut states = HashMap::new();
        for chunk in timestamps.chunks(500) {
            let sql = format!(
                "SELECT ts, state FROM petunia_send WHERE ts IN ({})",
                placeholders(chunk.len())
            );
            let mut query = sqlx::query(AssertSqlSafe(sql));
            for &timestamp in chunk {
                query = query.bind(timestamp as i64);
            }
            for row in query.fetch_all(&self.pool).await? {
                if let Some(status) = from_int(row.get(1)) {
                    states.insert(row.get::<i64, _>(0) as u64, status);
                }
            }
        }
        Ok(states)
    }

    async fn receipts(&self, timestamps: &[u64]) -> Result<HashMap<u64, Vec<Status>>, Error> {
        let mut receipts: HashMap<u64, Vec<Status>> = HashMap::new();
        for chunk in timestamps.chunks(500) {
            let sql = format!(
                "SELECT ts, state FROM petunia_receipt WHERE ts IN ({})",
                placeholders(chunk.len())
            );
            let mut query = sqlx::query(AssertSqlSafe(sql));
            for &timestamp in chunk {
                query = query.bind(timestamp as i64);
            }
            for row in query.fetch_all(&self.pool).await? {
                if let Some(status) = from_int(row.get(1)) {
                    receipts
                        .entry(row.get::<i64, _>(0) as u64)
                        .or_default()
                        .push(status);
                }
            }
        }
        Ok(receipts)
    }
}

/// Delivery and read only count once every recipient has reported, which is why
/// the receipts are kept per recipient; short of that the message is just sent.
fn aggregate(send: Option<Status>, receipts: &[Status], recipients: usize) -> Option<Status> {
    if let Some(status @ (Status::Sending | Status::Failed)) = send {
        return Some(status);
    }

    if receipts.is_empty() {
        return send.map(|_| Status::Sent);
    }

    let complete = receipts.len() >= recipients.max(1);
    let lowest = receipts.iter().copied().min().unwrap_or(Status::Sent);

    Some(if complete {
        lowest.max(Status::Sent)
    } else {
        Status::Sent
    })
}

fn thread_key(thread: &Thread) -> Vec<u8> {
    match thread {
        Thread::Contact(contact) => contact.uuid().as_bytes().to_vec(),
        Thread::Group(master_key) => master_key.to_vec(),
    }
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn to_int(status: Status) -> i64 {
    match status {
        Status::Sending => 0,
        Status::Failed => 1,
        Status::Sent => 2,
        Status::Delivered => 3,
        Status::Read => 4,
        Status::Viewed => 5,
    }
}

fn from_int(value: i64) -> Option<Status> {
    match value {
        0 => Some(Status::Sending),
        1 => Some(Status::Failed),
        2 => Some(Status::Sent),
        3 => Some(Status::Delivered),
        4 => Some(Status::Read),
        5 => Some(Status::Viewed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ContactId;

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    #[test]
    fn an_in_flight_send_still_reads_as_sending() {
        assert_eq!(
            aggregate(Some(Status::Sending), &[], 1),
            Some(Status::Sending)
        );
    }

    #[test]
    fn a_failed_send_stays_failed_even_with_receipts() {
        assert_eq!(
            aggregate(Some(Status::Failed), &[Status::Read], 1),
            Some(Status::Failed)
        );
    }

    #[test]
    fn a_sent_message_without_receipts_is_sent() {
        assert_eq!(aggregate(Some(Status::Sent), &[], 1), Some(Status::Sent));
    }

    #[test]
    fn an_unknown_message_has_no_status() {
        assert_eq!(aggregate(None, &[], 1), None);
    }

    #[test]
    fn one_to_one_delivery_needs_a_single_receipt() {
        assert_eq!(
            aggregate(Some(Status::Sent), &[Status::Delivered], 1),
            Some(Status::Delivered)
        );
        assert_eq!(
            aggregate(Some(Status::Sent), &[Status::Read], 1),
            Some(Status::Read)
        );
    }

    #[test]
    fn a_group_reports_sent_until_every_member_reports() {
        assert_eq!(
            aggregate(Some(Status::Sent), &[Status::Read], 3),
            Some(Status::Sent)
        );
        assert_eq!(
            aggregate(Some(Status::Sent), &[Status::Read, Status::Read], 3),
            Some(Status::Sent)
        );
    }

    #[test]
    fn a_group_reports_the_least_advanced_member() {
        assert_eq!(
            aggregate(
                Some(Status::Sent),
                &[Status::Read, Status::Read, Status::Delivered],
                3
            ),
            Some(Status::Delivered)
        );
        assert_eq!(
            aggregate(
                Some(Status::Sent),
                &[Status::Read, Status::Read, Status::Read],
                3
            ),
            Some(Status::Read)
        );
    }

    #[test]
    fn a_receipt_never_drags_a_status_below_sent() {
        assert_eq!(
            aggregate(Some(Status::Sent), &[Status::Sending], 1),
            Some(Status::Sent)
        );
    }

    #[tokio::test]
    async fn migrates_and_records_a_one_to_one_receipt() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let recipient = Uuid::new_v4();

        db.set_send_state(100, &thread, Status::Sent).await.unwrap();
        db.record_receipts(&[100], recipient, Status::Delivered)
            .await
            .unwrap();

        let statuses = db.statuses(&[100], 1).await.unwrap();
        assert_eq!(statuses.get(&100), Some(&Status::Delivered));
    }

    #[tokio::test]
    async fn keeps_group_receipts_apart_by_recipient() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_send_state(100, &thread, Status::Sent).await.unwrap();
        db.record_receipts(&[100], Uuid::new_v4(), Status::Read)
            .await
            .unwrap();

        // One of three members has read it, so the message is still just sent.
        assert_eq!(
            db.statuses(&[100], 3).await.unwrap().get(&100),
            Some(&Status::Sent)
        );

        db.record_receipts(&[100], Uuid::new_v4(), Status::Read)
            .await
            .unwrap();
        db.record_receipts(&[100], Uuid::new_v4(), Status::Delivered)
            .await
            .unwrap();

        assert_eq!(
            db.statuses(&[100], 3).await.unwrap().get(&100),
            Some(&Status::Delivered)
        );
    }

    #[tokio::test]
    async fn a_receipt_from_one_recipient_never_moves_backwards() {
        let db = Db::open_in_memory().await.unwrap();
        let recipient = Uuid::new_v4();
        db.set_send_state(100, &thread(), Status::Sent).await.unwrap();

        db.record_receipts(&[100], recipient, Status::Read)
            .await
            .unwrap();
        db.record_receipts(&[100], recipient, Status::Delivered)
            .await
            .unwrap();

        assert_eq!(
            db.statuses(&[100], 1).await.unwrap().get(&100),
            Some(&Status::Read)
        );
    }

    #[tokio::test]
    async fn drops_the_legacy_table_on_a_fresh_database() {
        let db = Db::open_in_memory().await.unwrap();

        assert_eq!(db.version().await.unwrap(), super::super::latest_version());
        assert!(!db.table_exists("petunia_message_status").await.unwrap());
        assert!(db.statuses(&[1, 2], 1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn back_fills_send_state_and_receipts_from_the_legacy_table() {
        let db = Db::open_in_memory_unmigrated().await.unwrap();
        db.seed_legacy_status(1, 0).await.unwrap(); // Sending
        db.seed_legacy_status(2, 1).await.unwrap(); // Failed
        db.seed_legacy_status(3, 2).await.unwrap(); // Sent
        db.seed_legacy_status(4, 3).await.unwrap(); // Delivered
        db.seed_legacy_status(5, 4).await.unwrap(); // Read

        db.migrate().await.unwrap();

        let statuses = db.statuses(&[1, 2, 3, 4, 5], 1).await.unwrap();
        assert_eq!(statuses.get(&1), Some(&Status::Sending));
        assert_eq!(statuses.get(&2), Some(&Status::Failed));
        assert_eq!(statuses.get(&3), Some(&Status::Sent));
        assert_eq!(statuses.get(&4), Some(&Status::Delivered));
        assert_eq!(statuses.get(&5), Some(&Status::Read));
        assert!(!db.table_exists("petunia_message_status").await.unwrap());
    }

    #[tokio::test]
    async fn sweeps_sends_left_in_flight_by_a_previous_run() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        db.set_send_state(100, &thread, Status::Sending).await.unwrap();
        db.set_send_state(200, &thread, Status::Sent).await.unwrap();

        assert_eq!(db.fail_stale_sends().await.unwrap(), 1);

        let statuses = db.statuses(&[100, 200], 1).await.unwrap();
        assert_eq!(statuses.get(&100), Some(&Status::Failed));
        assert_eq!(statuses.get(&200), Some(&Status::Sent));
    }

    /// The case a real install hits: a database already at an older version gains
    /// the new step without losing what the earlier one back-filled.
    #[tokio::test]
    async fn an_existing_database_migrates_forward_without_loss() {
        let db = Db::open_in_memory_unmigrated().await.unwrap();
        db.seed_legacy_status(7, 4).await.unwrap(); // Read
        db.migrate_upto(1).await.unwrap();

        assert_eq!(db.version().await.unwrap(), 1);
        assert!(!db.table_exists("petunia_blob").await.unwrap());

        db.migrate().await.unwrap();

        assert_eq!(db.version().await.unwrap(), super::super::latest_version());
        assert!(db.table_exists("petunia_blob").await.unwrap());
        assert_eq!(
            db.statuses(&[7], 1).await.unwrap().get(&7),
            Some(&Status::Read)
        );
    }

    #[tokio::test]
    async fn migrating_twice_is_a_no_op() {
        let db = Db::open_in_memory().await.unwrap();
        db.migrate().await.unwrap();

        assert_eq!(db.version().await.unwrap(), super::super::latest_version());
    }
}
