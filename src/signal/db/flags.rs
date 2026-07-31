//! What this device thinks about the conversation list.
//!
//! Pins and archives live in Signal's Storage Service, which libsignal-service
//! exposes read-only and presage neither uses nor exposes; folders it has no
//! concept of. So these are local, and the honest consequence is that they do
//! not follow you to your phone. What they do is survive a restart, which is the
//! difference between a setting and a gesture.

use sqlx::Row;

use super::Db;
use crate::data::index::Flags;
use crate::data::{ContactId, Thread};
use crate::signal::Error;

impl Db {
    pub async fn set_flags(&self, thread: &Thread, flags: &Flags) -> Result<(), Error> {
        // A conversation with nothing said about it is not a row. Keeping one
        // would leave the table full of defaults and make "is this pinned?"
        // depend on whether it was ever unpinned.
        if flags == &Flags::default() {
            sqlx::query("DELETE FROM petunia_thread_flags WHERE thread = ?")
                .bind(super::read::key(thread))
                .execute(&self.pool)
                .await?;
            return Ok(());
        }

        sqlx::query(
            "INSERT INTO petunia_thread_flags (thread, is_group, pinned, archived, muted_until, folder)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT DO UPDATE SET
                pinned = excluded.pinned,
                archived = excluded.archived,
                muted_until = excluded.muted_until,
                folder = excluded.folder",
        )
        .bind(super::read::key(thread))
        .bind(matches!(thread, Thread::Group(_)))
        .bind(flags.pinned)
        .bind(flags.archived)
        .bind(flags.muted_until.map(|until| until as i64))
        .bind(flags.folder.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Everything the list has been told, for the sidebar to start from.
    pub async fn flags(&self) -> Result<Vec<(Thread, Flags)>, Error> {
        let rows = sqlx::query(
            "SELECT thread, is_group, pinned, archived, muted_until, folder
            FROM petunia_thread_flags",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().filter_map(read).collect())
    }
}

fn read(row: &sqlx::sqlite::SqliteRow) -> Option<(Thread, Flags)> {
    let key: Vec<u8> = row.get(0);
    let thread = match row.get::<bool, _>(1) {
        true => Thread::Group(key.try_into().ok()?),
        false => Thread::Contact(ContactId::Aci(uuid::Uuid::from_slice(&key).ok()?)),
    };

    Some((
        thread,
        Flags {
            pinned: row.get(2),
            archived: row.get(3),
            muted_until: row.get::<Option<i64>, _>(4).map(|until| until as u64),
            folder: row.get::<Option<String>, _>(5),
            // Neither is stored: a block is Signal's to keep and a message
            // request is derived from whether a conversation has been answered.
            blocked: false,
            request: false,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    fn pinned() -> Flags {
        Flags {
            pinned: true,
            ..Flags::default()
        }
    }

    #[tokio::test]
    async fn nothing_is_flagged_to_begin_with() {
        let db = Db::open_in_memory().await.unwrap();

        assert!(db.flags().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn flags_round_trip() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let wanted = Flags {
            pinned: true,
            archived: false,
            muted_until: Some(1_700),
            folder: Some("Work".into()),
            ..Flags::default()
        };

        db.set_flags(&thread, &wanted).await.unwrap();

        let stored = db.flags().await.unwrap();
        assert_eq!(stored, [(thread, wanted)]);
    }

    #[tokio::test]
    async fn a_group_round_trips_its_thread() {
        let db = Db::open_in_memory().await.unwrap();
        let group = Thread::Group([9; 32]);

        db.set_flags(&group, &pinned()).await.unwrap();

        assert_eq!(db.flags().await.unwrap()[0].0, group);
    }

    #[tokio::test]
    async fn setting_flags_twice_updates_rather_than_duplicates() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_flags(&thread, &pinned()).await.unwrap();
        db.set_flags(
            &thread,
            &Flags {
                archived: true,
                ..Flags::default()
            },
        )
        .await
        .unwrap();

        let stored = db.flags().await.unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].1.archived);
        assert!(!stored[0].1.pinned);
    }

    /// Clearing everything removes the row rather than storing a row of
    /// defaults, or the table fills with conversations nothing was said about.
    #[tokio::test]
    async fn clearing_every_flag_forgets_the_thread() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_flags(&thread, &pinned()).await.unwrap();
        db.set_flags(&thread, &Flags::default()).await.unwrap();

        assert!(db.flags().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn threads_keep_their_own_flags() {
        let db = Db::open_in_memory().await.unwrap();
        let (a, b) = (thread(), thread());

        db.set_flags(&a, &pinned()).await.unwrap();
        db.set_flags(
            &b,
            &Flags {
                folder: Some("Family".into()),
                ..Flags::default()
            },
        )
        .await
        .unwrap();

        let stored = db.flags().await.unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored.iter().any(|(thread, flags)| *thread == a && flags.pinned));
        assert!(
            stored
                .iter()
                .any(|(thread, flags)| *thread == b && flags.folder.as_deref() == Some("Family"))
        );
    }
}
