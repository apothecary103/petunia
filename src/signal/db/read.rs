//! How far each conversation has been read.
//!
//! A watermark rather than a set: Signal's own read state is "everything up to
//! here", and a message older than the mark is read whether or not a receipt for
//! it was ever seen on this device. Without it the unread counts reset on every
//! restart, and reading on the phone never clears anything here.

use std::collections::HashMap;

use sqlx::Row;

use super::Db;
use crate::data::Thread;
use crate::signal::Error;

impl Db {
    /// Moves a thread's read mark forward. Never backwards: a stale sync must
    /// not make read messages unread again.
    pub async fn mark_read(&self, thread: &Thread, upto: u64) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_read (thread, read_upto) VALUES (?, ?)
            ON CONFLICT DO UPDATE SET read_upto = MAX(read_upto, excluded.read_upto)",
        )
        .bind(key(thread))
        .bind(upto as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The read mark for every thread that has one, which is what the sidebar
    /// counts unread messages against on startup.
    pub async fn read_marks(&self) -> Result<HashMap<Vec<u8>, u64>, Error> {
        let rows = sqlx::query("SELECT thread, read_upto FROM petunia_read")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| (row.get::<Vec<u8>, _>(0), row.get::<i64, _>(1) as u64))
            .collect())
    }

    /// How many messages in a thread arrived after its read mark and were not
    /// ours. A thread nobody has ever read has no mark, and everything in it
    /// counts.
    pub async fn unread(&self, thread: &Thread, aci: uuid::Uuid, upto: u64) -> Result<u32, Error> {
        let count: i64 = sqlx::query(
            "SELECT COUNT(*) FROM thread_messages m
            WHERE m.thread_id = (
                SELECT id FROM threads WHERE group_master_key = ?1 OR recipient_id = ?2
            )
            AND m.ts > ?3
            AND m.sender_service_id != ?4",
        )
        .bind(match thread {
            Thread::Group(master_key) => Some(master_key.to_vec()),
            Thread::Contact(_) => None,
        })
        .bind(match thread {
            Thread::Contact(contact) => Some(contact.uuid().to_string()),
            Thread::Group(_) => None,
        })
        .bind(upto as i64)
        .bind(aci.to_string())
        .fetch_one(&self.pool)
        .await?
        .get(0);

        Ok(count.max(0) as u32)
    }
}

/// The same key the receipt tables use, so a thread means one thing everywhere.
pub fn key(thread: &Thread) -> Vec<u8> {
    match thread {
        Thread::Contact(contact) => contact.uuid().as_bytes().to_vec(),
        Thread::Group(master_key) => master_key.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ContactId;
    use uuid::Uuid;

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    #[tokio::test]
    async fn an_unread_thread_has_no_mark() {
        let db = Db::open_in_memory().await.unwrap();

        assert!(db.read_marks().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_mark_round_trips() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.mark_read(&thread, 1_700).await.unwrap();

        assert_eq!(db.read_marks().await.unwrap().get(&key(&thread)), Some(&1_700));
    }

    /// A sync can arrive out of order, and honouring an older one would make
    /// read messages unread again.
    #[tokio::test]
    async fn a_mark_never_moves_backwards() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.mark_read(&thread, 2_000).await.unwrap();
        db.mark_read(&thread, 1_000).await.unwrap();

        assert_eq!(db.read_marks().await.unwrap().get(&key(&thread)), Some(&2_000));
    }

    #[tokio::test]
    async fn threads_are_marked_independently() {
        let db = Db::open_in_memory().await.unwrap();
        let (a, b) = (thread(), thread());

        db.mark_read(&a, 100).await.unwrap();
        db.mark_read(&b, 200).await.unwrap();

        let marks = db.read_marks().await.unwrap();
        assert_eq!(marks.get(&key(&a)), Some(&100));
        assert_eq!(marks.get(&key(&b)), Some(&200));
    }
}
