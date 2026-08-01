//! What people are called, kept across launches.
//!
//! Three sources and no winner among them: a nickname this account typed, the
//! name a profile carries, and the note written under it. Which one a view shows
//! is the view's decision, so all three are stored side by side rather than
//! folded into one string that would have to be unfolded to update half of it.
//!
//! None of this is authoritative -- Signal's own copies are -- but all of it is
//! *reachable without the network*, which is the point. A profile is one round
//! trip per person and the address book is fetched serially; a launch that ends
//! before that finishes used to leave everybody it had not reached as eight
//! characters of their uuid, with nothing kept for the next launch to start from.

use sqlx::Row;
use uuid::Uuid;

use super::Db;
use crate::Error;

/// What is known about one person, from every source at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub uuid: Uuid,
    pub profile: Option<String>,
    pub nickname: Option<String>,
    pub note: Option<String>,
}

impl Db {
    /// Records the name someone's profile carries. Only this column: a profile
    /// refresh that cleared the nickname beside it would undo a decision the
    /// user made, with a value the server never had an opinion about.
    pub async fn set_profile_name(&self, uuid: Uuid, name: &str) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_name (uuid, profile) VALUES (?, ?)
            ON CONFLICT DO UPDATE SET profile = excluded.profile",
        )
        .bind(uuid.as_bytes().as_slice())
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records a nickname and its note, either of which may be `None` -- which
    /// means cleared, not unread, so both are written whatever they hold.
    pub async fn set_nickname(
        &self,
        uuid: Uuid,
        name: Option<&str>,
        note: Option<&str>,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_name (uuid, nickname, note) VALUES (?, ?, ?)
            ON CONFLICT DO UPDATE SET
                nickname = excluded.nickname,
                note = excluded.note",
        )
        .bind(uuid.as_bytes().as_slice())
        .bind(name)
        .bind(note)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Everyone anything is known about, for the list to open with before a
    /// single profile has been fetched.
    pub async fn names(&self) -> Result<Vec<Known>, Error> {
        let rows = sqlx::query("SELECT uuid, profile, nickname, note FROM petunia_name")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().filter_map(read).collect())
    }
}

fn read(row: &sqlx::sqlite::SqliteRow) -> Option<Known> {
    let uuid = Uuid::from_slice(&row.get::<Vec<u8>, _>(0)).ok()?;
    Some(Known {
        uuid,
        profile: row.get(1),
        nickname: row.get(2),
        note: row.get(3),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn known(db: &Db, uuid: Uuid) -> Known {
        db.names()
            .await
            .unwrap()
            .into_iter()
            .find(|known| known.uuid == uuid)
            .unwrap()
    }

    #[tokio::test]
    async fn nobody_is_known_to_begin_with() {
        let db = Db::open_in_memory().await.unwrap();

        assert!(db.names().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_profile_name_round_trips() {
        let db = Db::open_in_memory().await.unwrap();
        let uuid = Uuid::new_v4();

        db.set_profile_name(uuid, "Alice").await.unwrap();

        assert_eq!(known(&db, uuid).await.profile.as_deref(), Some("Alice"));
    }

    #[tokio::test]
    async fn a_newer_profile_name_replaces_the_old_one() {
        let db = Db::open_in_memory().await.unwrap();
        let uuid = Uuid::new_v4();

        db.set_profile_name(uuid, "Alice").await.unwrap();
        db.set_profile_name(uuid, "Alice Cooper").await.unwrap();

        assert_eq!(
            known(&db, uuid).await.profile.as_deref(),
            Some("Alice Cooper")
        );
    }

    /// The one that matters: the two sources have to be independent, or a
    /// profile refresh silently undoes a nickname the user chose.
    #[tokio::test]
    async fn a_profile_and_a_nickname_do_not_disturb_each_other() {
        let db = Db::open_in_memory().await.unwrap();
        let uuid = Uuid::new_v4();

        db.set_nickname(uuid, Some("Mum"), Some("do not call after nine"))
            .await
            .unwrap();
        db.set_profile_name(uuid, "Alice").await.unwrap();

        let known = known(&db, uuid).await;
        assert_eq!(known.profile.as_deref(), Some("Alice"));
        assert_eq!(known.nickname.as_deref(), Some("Mum"));
        assert_eq!(known.note.as_deref(), Some("do not call after nine"));
    }

    #[tokio::test]
    async fn a_nickname_arriving_second_keeps_the_profile_name() {
        let db = Db::open_in_memory().await.unwrap();
        let uuid = Uuid::new_v4();

        db.set_profile_name(uuid, "Alice").await.unwrap();
        db.set_nickname(uuid, Some("Mum"), None).await.unwrap();

        let known = known(&db, uuid).await;
        assert_eq!(known.profile.as_deref(), Some("Alice"));
        assert_eq!(known.nickname.as_deref(), Some("Mum"));
    }

    /// `None` is a nickname that was taken away, which has to be written rather
    /// than skipped -- otherwise clearing one on the phone never reaches here.
    #[tokio::test]
    async fn a_cleared_nickname_is_stored_as_cleared() {
        let db = Db::open_in_memory().await.unwrap();
        let uuid = Uuid::new_v4();

        db.set_nickname(uuid, Some("Mum"), Some("a note")).await.unwrap();
        db.set_nickname(uuid, None, None).await.unwrap();

        let known = known(&db, uuid).await;
        assert_eq!(known.nickname, None);
        assert_eq!(known.note, None);
    }

    #[tokio::test]
    async fn people_are_known_independently() {
        let db = Db::open_in_memory().await.unwrap();
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());

        db.set_profile_name(alice, "Alice").await.unwrap();
        db.set_profile_name(bob, "Bob").await.unwrap();

        assert_eq!(db.names().await.unwrap().len(), 2);
        assert_eq!(known(&db, alice).await.profile.as_deref(), Some("Alice"));
        assert_eq!(known(&db, bob).await.profile.as_deref(), Some("Bob"));
    }
}
