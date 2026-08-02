//! Forgetting a conversation, and forgetting one message in one.
//!
//! Deleting a *conversation* is local only, like the flags: Signal's own
//! conversation delete travels through the Storage Service, which presage
//! neither uses nor exposes. So this removes what *this* device holds and says so
//! where it is asked for — it does not reach the phone, and the conversation
//! comes back the moment anyone says anything in it.
//!
//! Deleting one *message* is not in that position. There is a sync message for
//! it, so `delete_message` here is half of the job and `outgoing::delete_for_me`
//! is the other: the row goes from this device, and the account's other devices
//! are told to do the same.
//!
//! Every table that keys on a thread is cleared here, which is why they are
//! listed in one place: a table added later and forgotten would leave rows behind
//! that nothing would ever look at again.
//!
//! The cached media is deliberately left alone. It is content-addressed, so the
//! same picture in another conversation is the same file, and it is already
//! bounded by `cache_limit` — deleting a conversation's attachments would mean
//! deciding which of them nothing else refers to, for a directory that prunes
//! itself anyway.

use sqlx::Row;

use super::Db;
use petunia_data::Thread;
use crate::Error;

impl Db {
    /// Drops a conversation's messages and everything petunia records about it.
    /// Reports how many messages went, so the log says what happened.
    pub async fn delete_thread(&self, thread: &Thread) -> Result<u64, Error> {
        let key = super::read::key(thread);
        let (master_key, recipient) = match thread {
            Thread::Contact(contact) => (None, Some(contact.uuid())),
            Thread::Group(master_key) => (Some(master_key.to_vec()), None),
        };

        let mut tx = self.pool.begin().await?;

        // presage's own row for the thread, found the same way every read finds
        // it. Taken first: the timestamps are what the per-message tables key on,
        // and once the messages are gone there is no way to know which they were.
        let id: Option<i64> = sqlx::query(
            "SELECT id FROM threads WHERE group_master_key = ?1 OR recipient_id = ?2",
        )
        .bind(&master_key)
        .bind(recipient)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.get(0));

        let mut messages = 0;
        if let Some(id) = id {
            let timestamps: Vec<i64> =
                sqlx::query("SELECT ts FROM thread_messages WHERE thread_id = ?")
                    .bind(id)
                    .fetch_all(&mut *tx)
                    .await?
                    .iter()
                    .map(|row| row.get(0))
                    .collect();

            // Receipts key on the timestamp alone, because a ReceiptMessage names
            // no thread. They can only be found through the messages they belong
            // to, so they go before them.
            for ts in &timestamps {
                sqlx::query("DELETE FROM petunia_receipt WHERE ts = ?")
                    .bind(ts)
                    .execute(&mut *tx)
                    .await?;
            }

            messages = sqlx::query("DELETE FROM thread_messages WHERE thread_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();

            // The thread row itself, so the conversation stops being listed at
            // all rather than being listed as empty.
            sqlx::query("DELETE FROM threads WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        // Written out rather than looped over a list of names, because sqlx will
        // not take a query string it cannot see at compile time -- and because a
        // reader looking for "what does petunia keep about a conversation" finds
        // the answer here.
        for statement in [
            "DELETE FROM petunia_body WHERE thread = ?",
            "DELETE FROM petunia_send WHERE thread = ?",
            "DELETE FROM petunia_thread_flags WHERE thread = ?",
            "DELETE FROM petunia_read WHERE thread = ?",
            "DELETE FROM petunia_avatar WHERE thread = ?",
            "DELETE FROM petunia_preview WHERE thread = ?",
        ] {
            sqlx::query(statement)
                .bind(&key)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(messages)
    }

    /// Drops one message from this device: the stored row, the search index and
    /// the receipt keyed on it.
    ///
    /// Not a tombstone. A remote delete leaves one, because everybody in the
    /// conversation has to be told the message was withdrawn -- but "delete for
    /// me" is the reader deciding they do not want to see it, and a line reading
    /// "this message was deleted" where they asked for nothing is not that.
    pub async fn delete_message(&self, thread: &Thread, timestamp: u64) -> Result<bool, Error> {
        let key = super::read::key(thread);
        let ts = timestamp as i64;
        let mut tx = self.pool.begin().await?;

        let id: Option<i64> = sqlx::query(
            "SELECT thread_id FROM thread_messages WHERE ts = ?1 AND thread_id IN \
             (SELECT id FROM threads WHERE group_master_key = ?2 OR recipient_id = ?3)",
        )
        .bind(ts)
        .bind(match thread {
            Thread::Group(master_key) => Some(master_key.to_vec()),
            Thread::Contact(_) => None,
        })
        .bind(match thread {
            Thread::Contact(contact) => Some(contact.uuid()),
            Thread::Group(_) => None,
        })
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.get(0));

        let gone = match id {
            Some(id) => {
                sqlx::query("DELETE FROM thread_messages WHERE thread_id = ?1 AND ts = ?2")
                    .bind(id)
                    .bind(ts)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
                    > 0
            }
            None => false,
        };

        for statement in [
            "DELETE FROM petunia_body WHERE thread = ?1 AND ts = ?2",
            "DELETE FROM petunia_send WHERE thread = ?1 AND ts = ?2",
        ] {
            sqlx::query(statement)
                .bind(&key)
                .bind(ts)
                .execute(&mut *tx)
                .await?;
        }
        // Keyed on the timestamp alone, because a ReceiptMessage names no thread.
        sqlx::query("DELETE FROM petunia_receipt WHERE ts = ?")
            .bind(ts)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(gone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use petunia_data::{ContactId, Status};
    use uuid::Uuid;

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    /// A conversation with a message in it, a read mark, flags, a send state and
    /// a search entry -- one of everything that keys on a thread.
    async fn seeded(db: &Db, thread: &Thread, ts: u64) {
        let sender = Uuid::new_v4();
        db.index_bodies(thread, &[(ts, sender, "hi".into())])
            .await
            .unwrap();
        db.set_preview(thread, &petunia_data::index::Preview::new("hi".into(), ts, None))
            .await
            .unwrap();
        db.mark_read(thread, ts).await.unwrap();
        db.set_send_state(ts, thread, Status::Sent).await.unwrap();
        db.set_avatar_source(thread, "cdn/one").await.unwrap();
        db.set_flags(
            thread,
            &petunia_data::index::Flags {
                pinned: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn deleting_a_conversation_leaves_nothing_of_it() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        seeded(&db, &thread, 100).await;

        db.delete_thread(&thread).await.unwrap();

        assert!(db.search("hi", None).await.unwrap().is_empty());
        assert!(db.read_marks().await.unwrap().is_empty());
        assert!(db.flags().await.unwrap().is_empty());
        assert!(db.previews().await.unwrap().is_empty());
        assert_eq!(db.avatar_source(&thread).await.unwrap(), None);
    }

    /// The one that matters: deleting one conversation must not touch another.
    #[tokio::test]
    async fn another_conversation_is_left_alone() {
        let db = Db::open_in_memory().await.unwrap();
        let (gone, kept) = (thread(), thread());
        seeded(&db, &gone, 100).await;
        seeded(&db, &kept, 200).await;

        db.delete_thread(&gone).await.unwrap();

        assert_eq!(
            db.read_marks().await.unwrap().get(&super::super::read::key(&kept)),
            Some(&200)
        );
        assert_eq!(db.flags().await.unwrap().len(), 1);
        assert_eq!(db.previews().await.unwrap().len(), 1);
        assert_eq!(db.avatar_source(&kept).await.unwrap(), Some("cdn/one".into()));
        assert_eq!(db.search("hi", None).await.unwrap().len(), 1);
    }

    /// A group is addressed by its master key rather than by a uuid, so it takes
    /// the other branch of every lookup here.
    #[tokio::test]
    async fn a_group_can_be_deleted_too() {
        let db = Db::open_in_memory().await.unwrap();
        let group = Thread::Group([9; 32]);
        seeded(&db, &group, 100).await;

        db.delete_thread(&group).await.unwrap();

        assert!(db.flags().await.unwrap().is_empty());
        assert!(db.search("hi", None).await.unwrap().is_empty());
    }

    /// Deleting a conversation that was never stored is not an error: the list can
    /// show a contact that has no thread row behind it yet.
    #[tokio::test]
    async fn deleting_an_unknown_conversation_is_harmless() {
        let db = Db::open_in_memory().await.unwrap();

        assert_eq!(db.delete_thread(&thread()).await.unwrap(), 0);
    }
}

