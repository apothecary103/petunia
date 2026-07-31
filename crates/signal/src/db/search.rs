//! Finding a message again.
//!
//! Searching means reading bodies, and a body only exists inside an encoded
//! protobuf in presage's `thread_messages`. There is no column to match on, so
//! petunia keeps its own index of what messages said — written as threads are
//! read, which is also when the rows are already decoded and in hand.
//!
//! `LIKE` rather than FTS5: whether sqlite was built with FTS5 is not knowable
//! until runtime, and at the scale of one person's message history a scan of an
//! indexed table is not the slow part. If that stops being true, the table is
//! the right shape to build an FTS index over.

use sqlx::{AssertSqlSafe, Row};
use uuid::Uuid;

use super::Db;
use petunia_data::{ContactId, Thread};
use crate::Error;

/// One message that matched, with enough context to draw a row and open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub thread: Thread,
    pub sender: Uuid,
    pub timestamp: u64,
    pub body: String,
}

/// How many matches to return. A search box that lists a thousand rows is a
/// worse answer than one that lists the newest fifty.
pub const LIMIT: u32 = 50;

impl Db {
    /// Records what a message said, so it can be found later. Called with a page
    /// of history, where the bodies have already been decoded.
    pub async fn index_bodies(
        &self,
        thread: &Thread,
        messages: &[(u64, Uuid, String)],
    ) -> Result<(), Error> {
        if messages.is_empty() {
            return Ok(());
        }
        let key = super::read::key(thread);
        let group = matches!(thread, Thread::Group(_));

        let mut tx = self.pool.begin().await?;
        for (timestamp, sender, body) in messages {
            if body.trim().is_empty() {
                continue;
            }
            sqlx::query(
                "INSERT INTO petunia_body (ts, thread, is_group, sender, body)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT DO UPDATE SET body = excluded.body",
            )
            .bind(*timestamp as i64)
            .bind(&key)
            .bind(group)
            .bind(sender.as_bytes().as_slice())
            .bind(body)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Messages matching a query, newest first. An empty query matches nothing:
    /// "everything you have ever said" is not a search result.
    pub async fn search(&self, query: &str, within: Option<&Thread>) -> Result<Vec<Hit>, Error> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let scoped = match within {
            Some(_) => "AND thread = ?2",
            None => "",
        };
        let sql = format!(
            "SELECT ts, thread, is_group, sender, body FROM petunia_body
            WHERE body LIKE ?1 ESCAPE '\\' {scoped}
            ORDER BY ts DESC LIMIT ?3"
        );

        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(format!("%{}%", escape(query)))
            .bind(within.map(super::read::key))
            .bind(i64::from(LIMIT))
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().filter_map(hit).collect())
    }
}

fn hit(row: &sqlx::sqlite::SqliteRow) -> Option<Hit> {
    let key: Vec<u8> = row.get(1);
    let thread = match row.get::<bool, _>(2) {
        true => Thread::Group(key.try_into().ok()?),
        false => Thread::Contact(ContactId::Aci(Uuid::from_slice(&key).ok()?)),
    };
    let sender: Vec<u8> = row.get(3);

    Some(Hit {
        thread,
        sender: Uuid::from_slice(&sender).ok()?,
        timestamp: row.get::<i64, _>(0) as u64,
        body: row.get(4),
    })
}

/// `LIKE` treats `%` and `_` as wildcards, so searching for "100%" or a
/// `snake_case` name would otherwise match far more than was asked for.
fn escape(query: &str) -> String {
    query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    async fn seeded() -> (Db, Thread, Thread, Uuid) {
        let db = Db::open_in_memory().await.unwrap();
        let (alice, group) = (thread(), Thread::Group([7; 32]));
        let sender = Uuid::new_v4();

        db.index_bodies(
            &alice,
            &[
                (100, sender, "the deploy went fine".into()),
                (200, sender, "lunch?".into()),
            ],
        )
        .await
        .unwrap();
        db.index_bodies(&group, &[(300, sender, "deploy is stuck".into())])
            .await
            .unwrap();

        (db, alice, group, sender)
    }

    #[tokio::test]
    async fn finds_a_word_across_every_conversation() {
        let (db, _, _, _) = seeded().await;

        let hits = db.search("deploy", None).await.unwrap();

        assert_eq!(hits.len(), 2);
        // Newest first, so the most likely answer is at the top.
        assert_eq!(hits[0].timestamp, 300);
    }

    #[tokio::test]
    async fn scoping_to_a_thread_leaves_the_others_out() {
        let (db, alice, _, _) = seeded().await;

        let hits = db.search("deploy", Some(&alice)).await.unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].thread, alice);
    }

    #[tokio::test]
    async fn a_group_hit_round_trips_its_thread() {
        let (db, _, group, _) = seeded().await;

        let hits = db.search("stuck", None).await.unwrap();

        assert_eq!(hits[0].thread, group);
    }

    #[tokio::test]
    async fn matching_ignores_case() {
        let (db, _, _, _) = seeded().await;

        assert_eq!(db.search("DEPLOY", None).await.unwrap().len(), 2);
    }

    /// "Everything you have ever said" is not a search result.
    #[tokio::test]
    async fn an_empty_query_finds_nothing() {
        let (db, _, _, _) = seeded().await;

        assert!(db.search("", None).await.unwrap().is_empty());
        assert!(db.search("   ", None).await.unwrap().is_empty());
    }

    /// `%` and `_` are wildcards to `LIKE`, and a person searching for one means
    /// the character.
    #[tokio::test]
    async fn wildcards_in_the_query_are_literal() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let sender = Uuid::new_v4();
        db.index_bodies(
            &thread,
            &[
                (1, sender, "battery at 100%".into()),
                (2, sender, "battery at 100 percent".into()),
            ],
        )
        .await
        .unwrap();

        let hits = db.search("100%", None).await.unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].timestamp, 1);
    }

    #[tokio::test]
    async fn an_underscore_is_not_a_wildcard_either() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let sender = Uuid::new_v4();
        db.index_bodies(
            &thread,
            &[(1, sender, "read_to_string".into()), (2, sender, "readXtoXstring".into())],
        )
        .await
        .unwrap();

        assert_eq!(db.search("read_to", None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reindexing_a_message_replaces_what_it_said() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let sender = Uuid::new_v4();

        db.index_bodies(&thread, &[(1, sender, "typo".into())]).await.unwrap();
        db.index_bodies(&thread, &[(1, sender, "fixed".into())]).await.unwrap();

        assert!(db.search("typo", None).await.unwrap().is_empty());
        assert_eq!(db.search("fixed", None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_empty_body_is_not_indexed() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let sender = Uuid::new_v4();

        db.index_bodies(&thread, &[(1, sender, "   ".into())]).await.unwrap();

        assert!(db.search(" ", None).await.unwrap().is_empty());
    }
}
