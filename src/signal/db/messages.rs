use chrono::{TimeZone, Utc};
use presage::libsignal_service::content::{Content as Envelope, Metadata};
use presage::libsignal_service::prelude::ProtobufMessage;
use presage::libsignal_service::proto;
use presage::libsignal_service::protocol::{DeviceId, ServiceId};
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row};
use uuid::Uuid;

use super::Db;
use crate::data::{ContactId, Thread};
use crate::signal::Error;

/// Rows without bubbles -- receipts, reactions, edits, sync -- consume page
/// slots, so a page is over-fetched and the caller folds it down.
const OVERFETCH: u32 = 3;

/// How far back to look for a thread's newest renderable message.
const PREVIEW_DEPTH: u32 = 20;

pub struct Page {
    pub rows: Vec<Envelope>,
    pub more: bool,
}

const COLUMNS: &str = "m.ts, m.server_ts, m.sender_service_id, m.sender_device_id,
    m.destination_service_id, m.needs_receipt, m.unidentified_sender, m.content_body,
    m.was_plaintext";

impl Db {
    /// One page of a thread, newest first in the query but returned oldest
    /// first. presage's own `messages()` takes a range with no limit and
    /// `fetch_all`s it, so paging has to happen here.
    pub async fn page(
        &self,
        thread: &Thread,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Page, Error> {
        let requested = limit.max(1).saturating_mul(OVERFETCH);
        let sql = format!(
            "SELECT {COLUMNS} FROM thread_messages m
            WHERE m.thread_id = (
                SELECT id FROM threads WHERE group_master_key = ?1 OR recipient_id = ?2
            )
            AND (?3 IS NULL OR m.ts < ?3)
            ORDER BY m.ts DESC
            LIMIT ?4"
        );

        let (master_key, recipient) = keys(thread);
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(master_key)
            .bind(recipient)
            .bind(before.map(|ts| ts as i64))
            .bind(i64::from(requested) + 1)
            .fetch_all(&self.pool)
            .await?;

        let more = rows.len() > requested as usize;
        let mut rows: Vec<Envelope> = rows
            .iter()
            .take(requested as usize)
            .filter_map(decode)
            .collect();
        rows.reverse();

        Ok(Page { rows, more })
    }

    /// A single stored row, so an attachment download can recover the pointer
    /// without it having been carried through the UI.
    pub async fn row(&self, thread: &Thread, timestamp: u64) -> Result<Option<Envelope>, Error> {
        let sql = format!(
            "SELECT {COLUMNS} FROM thread_messages m
            WHERE m.thread_id = (
                SELECT id FROM threads WHERE group_master_key = ?1 OR recipient_id = ?2
            )
            AND m.ts = ?3"
        );

        let (master_key, recipient) = keys(thread);
        Ok(sqlx::query(AssertSqlSafe(sql))
            .bind(master_key)
            .bind(recipient)
            .bind(timestamp as i64)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .and_then(decode))
    }

    /// The newest rows of every thread that has any, in one statement. The old
    /// path walked every contact and group and loaded each thread in full,
    /// which cost O(all messages) at startup.
    pub async fn previews(&self) -> Result<Vec<(Thread, Vec<Envelope>)>, Error> {
        // The window runs over (thread_id, ts) alone so it is answered from the
        // covering index; bodies are then fetched only for the rows that qualify.
        let sql = format!(
            "SELECT t.group_master_key, t.recipient_id, {COLUMNS}
            FROM (
                SELECT thread_id, ts, ROW_NUMBER() OVER (
                    PARTITION BY thread_id ORDER BY ts DESC
                ) AS rank FROM thread_messages
            ) ranked
            JOIN thread_messages m
                ON m.thread_id = ranked.thread_id AND m.ts = ranked.ts
            JOIN threads t ON t.id = m.thread_id
            WHERE ranked.rank <= ?1
            ORDER BY m.thread_id, m.ts"
        );

        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(i64::from(PREVIEW_DEPTH))
            .fetch_all(&self.pool)
            .await?;

        let mut threads: Vec<(Thread, Vec<Envelope>)> = Vec::new();
        for row in &rows {
            let Some(thread) = row_thread(row) else {
                continue;
            };
            let Some(envelope) = decode(row) else {
                continue;
            };
            match threads.last_mut() {
                Some((last, envelopes)) if *last == thread => envelopes.push(envelope),
                _ => threads.push((thread, vec![envelope])),
            }
        }
        Ok(threads)
    }
}

fn keys(thread: &Thread) -> (Option<Vec<u8>>, Option<Uuid>) {
    match thread {
        Thread::Contact(contact) => (None, Some(contact.uuid())),
        Thread::Group(master_key) => (Some(master_key.to_vec()), None),
    }
}

fn row_thread(row: &SqliteRow) -> Option<Thread> {
    if let Ok(Some(master_key)) = row.try_get::<Option<Vec<u8>>, _>("group_master_key")
        && let Ok(master_key) = <[u8; 32]>::try_from(master_key.as_slice())
    {
        return Some(Thread::Group(master_key));
    }
    let recipient: Option<Uuid> = row.try_get("recipient_id").ok()?;
    recipient.map(|uuid| Thread::Contact(ContactId::Aci(uuid)))
}

/// Mirrors presage-store-sqlite's own row decoding; every piece it uses is
/// public, but the conversion itself is not exposed.
fn decode(row: &SqliteRow) -> Option<Envelope> {
    let ts: i64 = row.try_get("ts").ok()?;
    let server_ts: Option<i64> = row.try_get("server_ts").ok()?;
    let body: Vec<u8> = row.try_get("content_body").ok()?;
    let sender: String = row.try_get("sender_service_id").ok()?;
    let destination: String = row.try_get("destination_service_id").ok()?;
    let device: i64 = row.try_get("sender_device_id").ok()?;

    let proto = proto::Content::decode(&*body).ok()?;
    let metadata = Metadata {
        sender: ServiceId::parse_from_service_id_string(&sender)?,
        destination: ServiceId::parse_from_service_id_string(&destination)?,
        sender_device: DeviceId::new(u8::try_from(device).ok()?).ok()?,
        timestamp: Utc.timestamp_millis_opt(ts).single()?,
        server_timestamp: Utc.timestamp_millis_opt(server_ts.unwrap_or(ts)).single()?,
        needs_receipt: row.try_get("needs_receipt").ok()?,
        unidentified_sender: row.try_get("unidentified_sender").ok()?,
        was_plaintext: row.try_get("was_plaintext").ok()?,
        server_guid: None,
    };

    Envelope::from_proto(proto, metadata).ok()
}

#[cfg(test)]
mod tests {
    use presage::libsignal_service::proto::DataMessage;
    use presage::store::ContentsStore;
    use presage_store_sqlite::{OnNewIdentity, SqliteStore};

    use super::*;
    use crate::data;

    /// The page query reads presage's own tables, so the fixture is built by
    /// asking presage to store real messages rather than by hand.
    async fn seeded(count: u64) -> (Db, Thread, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db3");
        let store = SqliteStore::open(path.to_str().unwrap(), OnNewIdentity::Trust)
            .await
            .unwrap();

        let sender = Uuid::new_v4();
        for index in 1..=count {
            let time = Utc.timestamp_millis_opt(index as i64 * 1000).unwrap();
            let envelope = Envelope::from_body(
                DataMessage {
                    body: Some(format!("message {index}")),
                    timestamp: Some(index * 1000),
                    ..Default::default()
                },
                Metadata {
                    sender: ServiceId::Aci(sender.into()),
                    destination: ServiceId::Aci(sender.into()),
                    sender_device: DeviceId::new(1).unwrap(),
                    timestamp: time,
                    server_timestamp: time,
                    needs_receipt: false,
                    unidentified_sender: false,
                    was_plaintext: false,
                    server_guid: None,
                },
            );
            let thread = presage::store::Thread::try_from(&envelope).unwrap();
            store.save_message(&thread, envelope).await.unwrap();
        }
        drop(store);

        let db = Db::open_at(path.to_str().unwrap()).await.unwrap();
        (db, Thread::Contact(ContactId::Aci(sender)), dir)
    }

    fn bodies(page: &Page) -> Vec<String> {
        data::project(page.rows.clone())
            .iter()
            .filter_map(|message| message.text().map(str::to_string))
            .collect()
    }

    #[tokio::test]
    async fn returns_the_newest_page_oldest_first() {
        let (db, thread, _dir) = seeded(5).await;
        let page = db.page(&thread, None, 100).await.unwrap();

        assert_eq!(
            bodies(&page),
            [
                "message 1",
                "message 2",
                "message 3",
                "message 4",
                "message 5"
            ]
        );
        assert!(!page.more);
    }

    #[tokio::test]
    async fn reports_more_when_the_thread_is_longer_than_a_page() {
        let (db, thread, _dir) = seeded(30).await;
        // The limit is over-fetched, so ask for a page far smaller than the thread.
        let page = db.page(&thread, None, 2).await.unwrap();

        assert!(page.more);
        assert_eq!(page.rows.len(), 6);
        assert_eq!(bodies(&page).last().unwrap(), "message 30");
    }

    #[tokio::test]
    async fn walks_backwards_with_before() {
        let (db, thread, _dir) = seeded(30).await;
        let newest = db.page(&thread, None, 2).await.unwrap();
        let oldest_seen = data::project(newest.rows.clone())
            .first()
            .unwrap()
            .timestamp();

        let older = db.page(&thread, Some(oldest_seen), 2).await.unwrap();

        let bodies = bodies(&older);
        assert!(page_is_older(&bodies, oldest_seen));
        assert!(older.more);
    }

    fn page_is_older(bodies: &[String], oldest_seen: u64) -> bool {
        let highest: u64 = bodies
            .last()
            .unwrap()
            .trim_start_matches("message ")
            .parse()
            .unwrap();
        highest * 1000 < oldest_seen
    }

    #[tokio::test]
    async fn an_unknown_thread_has_an_empty_page() {
        let (db, _, _dir) = seeded(3).await;
        let stranger = Thread::Contact(ContactId::Aci(Uuid::new_v4()));

        let page = db.page(&stranger, None, 50).await.unwrap();
        assert!(page.rows.is_empty());
        assert!(!page.more);
    }

    #[tokio::test]
    async fn previews_return_one_thread_with_its_newest_rows() {
        let (db, thread, _dir) = seeded(25).await;
        let previews = db.previews().await.unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].0, thread);

        // Bounded by PREVIEW_DEPTH, newest last so `project(..).pop()` is the latest.
        assert_eq!(previews[0].1.len(), PREVIEW_DEPTH as usize);
        let latest = data::project(previews[0].1.clone()).pop().unwrap();
        assert_eq!(latest.text(), Some("message 25"));
    }

    #[tokio::test]
    async fn previews_are_empty_without_messages() {
        let db = Db::open_in_memory().await.unwrap();
        assert!(db.previews().await.unwrap().is_empty());
    }
}
