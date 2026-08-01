//! The line the sidebar draws for each conversation, kept across launches.
//!
//! The line is written when the message arrives, which is the only moment it is
//! cheap: the message is in hand, already decoded and already projected. Reading
//! it back at startup is one query for the whole list, rather than a scan of
//! every thread's newest rows that has to project each one to find out whether
//! it says anything -- and that could answer "nothing" for a thread full of
//! messages, which is how people disappeared from the list.

use sqlx::Row;

use super::Db;
use petunia_data::index::Preview;
use petunia_data::{ContactId, Thread};
use crate::Error;

impl Db {
    /// Records what a thread's newest message reads as. Never backwards: pages
    /// of history arrive newest-first and an older one must not overwrite the
    /// line the sidebar is already showing.
    pub async fn set_preview(&self, thread: &Thread, preview: &Preview) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_preview (thread, is_group, at, line) VALUES (?, ?, ?, ?)
            ON CONFLICT DO UPDATE SET
                at = excluded.at,
                line = excluded.line
            WHERE excluded.at >= petunia_preview.at",
        )
        .bind(super::read::key(thread))
        .bind(matches!(thread, Thread::Group(_)))
        .bind(preview.at() as i64)
        .bind(&preview.line)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every line the list has been told, for the sidebar to open with.
    pub async fn previews(&self) -> Result<Vec<(Thread, Preview)>, Error> {
        let rows = sqlx::query("SELECT thread, is_group, at, line FROM petunia_preview")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().filter_map(read).collect())
    }

    /// Forgets a conversation's line, for when the conversation itself is
    /// forgotten. Its own statement rather than another entry in
    /// `delete_thread`'s list, because that list is written out for a reader
    /// asking what petunia keeps -- and this is the answer for the sidebar.
    pub async fn forget_preview(&self, thread: &Thread) -> Result<(), Error> {
        sqlx::query("DELETE FROM petunia_preview WHERE thread = ?")
            .bind(super::read::key(thread))
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn read(row: &sqlx::sqlite::SqliteRow) -> Option<(Thread, Preview)> {
    let key: Vec<u8> = row.get(0);
    let thread = match row.get::<bool, _>(1) {
        true => Thread::Group(key.try_into().ok()?),
        false => Thread::Contact(ContactId::Aci(uuid::Uuid::from_slice(&key).ok()?)),
    };
    let at = row.get::<i64, _>(2) as u64;
    Some((thread, Preview::new(row.get(3), at)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    fn preview(line: &str, at: u64) -> Preview {
        Preview::new(line.into(), at)
    }

    #[tokio::test]
    async fn nothing_is_remembered_to_begin_with() {
        let db = Db::open_in_memory().await.unwrap();

        assert!(db.previews().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_line_round_trips() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_preview(&thread, &preview("see you then", 1_700))
            .await
            .unwrap();

        let stored = db.previews().await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, thread);
        assert_eq!(stored[0].1.line, "see you then");
        assert_eq!(stored[0].1.at(), 1_700);
    }

    #[tokio::test]
    async fn a_newer_line_replaces_the_one_before_it() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_preview(&thread, &preview("first", 100)).await.unwrap();
        db.set_preview(&thread, &preview("second", 200)).await.unwrap();

        assert_eq!(db.previews().await.unwrap()[0].1.line, "second");
    }

    /// A page of history is read newest-first, so an older line arriving after a
    /// newer one is the ordinary case rather than the odd one.
    #[tokio::test]
    async fn an_older_line_is_ignored() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_preview(&thread, &preview("newer", 200)).await.unwrap();
        db.set_preview(&thread, &preview("older", 100)).await.unwrap();

        let stored = db.previews().await.unwrap();
        assert_eq!(stored[0].1.line, "newer");
        assert_eq!(stored[0].1.at(), 200);
    }

    /// An edit rewrites the newest message without moving it, so the line has to
    /// be replaceable at the timestamp it already carries.
    #[tokio::test]
    async fn a_line_at_the_same_moment_replaces_the_one_there() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_preview(&thread, &preview("typo", 100)).await.unwrap();
        db.set_preview(&thread, &preview("fixed", 100)).await.unwrap();

        assert_eq!(db.previews().await.unwrap()[0].1.line, "fixed");
    }

    #[tokio::test]
    async fn a_group_keeps_its_own_line() {
        let db = Db::open_in_memory().await.unwrap();
        let group = Thread::Group([7; 32]);

        db.set_preview(&group, &preview("who is bringing what", 50))
            .await
            .unwrap();

        let stored = db.previews().await.unwrap();
        assert_eq!(stored[0].0, group);
        assert_eq!(stored[0].1.line, "who is bringing what");
    }

    #[tokio::test]
    async fn threads_keep_their_own_lines() {
        let db = Db::open_in_memory().await.unwrap();
        let (a, b) = (thread(), thread());

        db.set_preview(&a, &preview("one", 100)).await.unwrap();
        db.set_preview(&b, &preview("two", 200)).await.unwrap();

        let stored = db.previews().await.unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored.iter().any(|(t, p)| *t == a && p.line == "one"));
        assert!(stored.iter().any(|(t, p)| *t == b && p.line == "two"));
    }

    #[tokio::test]
    async fn forgetting_a_conversation_forgets_its_line() {
        let db = Db::open_in_memory().await.unwrap();
        let (gone, kept) = (thread(), thread());
        db.set_preview(&gone, &preview("one", 100)).await.unwrap();
        db.set_preview(&kept, &preview("two", 200)).await.unwrap();

        db.forget_preview(&gone).await.unwrap();

        let stored = db.previews().await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, kept);
    }
}
