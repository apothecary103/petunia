//! Keeping avatars current.
//!
//! There are three caches between a changed profile picture and the screen, and
//! none of them used to expire: presage stores the profile, presage stores the
//! decrypted avatar bytes, and petunia stores the file. A picture changed on
//! the phone therefore never arrived — every other client showed it fine.
//!
//! The fix is not to stop caching. It is to record *which* remote picture the
//! bytes came from, so a refresh costs one small profile fetch per person and a
//! download only when the answer has changed.

use sqlx::Row;

use super::Db;
use crate::data::Thread;
use crate::signal::Error;

impl Db {
    /// What the cached avatar for this thread was fetched from, if anything.
    pub async fn avatar_source(&self, thread: &Thread) -> Result<Option<String>, Error> {
        Ok(
            sqlx::query("SELECT remote FROM petunia_avatar WHERE thread = ?")
                .bind(super::read::key(thread))
                .fetch_optional(&self.pool)
                .await?
                .map(|row| row.get(0)),
        )
    }

    pub async fn set_avatar_source(&self, thread: &Thread, remote: &str) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_avatar (thread, remote) VALUES (?, ?)
            ON CONFLICT DO UPDATE SET remote = excluded.remote",
        )
        .bind(super::read::key(thread))
        .bind(remote)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Drops presage's own profile and avatar caches, which it has no way to
    /// invalidate (`registered.rs` says as much, twice, in a TODO).
    ///
    /// `ContentsStore::clear_profiles` would do this, but it also clears
    /// `profile_keys` — the sealed-sender access keys — and losing those on
    /// every launch would quietly downgrade how messages are sent until each
    /// one was learned again. These two tables are pure caches; that one is not.
    pub async fn forget_profiles(&self) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM profiles").execute(&mut *tx).await?;
        sqlx::query("DELETE FROM profile_avatars")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
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
    async fn an_unfetched_avatar_has_no_source() {
        let db = Db::open_in_memory().await.unwrap();

        assert_eq!(db.avatar_source(&thread()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_source_round_trips() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_avatar_source(&thread, "profiles/abc").await.unwrap();

        assert_eq!(
            db.avatar_source(&thread).await.unwrap().as_deref(),
            Some("profiles/abc")
        );
    }

    /// The whole point: a new picture has a new path, and noticing that is what
    /// makes the refresh cheap.
    #[tokio::test]
    async fn a_changed_source_replaces_the_old_one() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_avatar_source(&thread, "profiles/old").await.unwrap();
        db.set_avatar_source(&thread, "profiles/new").await.unwrap();

        assert_eq!(
            db.avatar_source(&thread).await.unwrap().as_deref(),
            Some("profiles/new")
        );
    }

    #[tokio::test]
    async fn threads_record_their_own_sources() {
        let db = Db::open_in_memory().await.unwrap();
        let (a, b) = (thread(), thread());

        db.set_avatar_source(&a, "one").await.unwrap();
        db.set_avatar_source(&b, "two").await.unwrap();

        assert_eq!(db.avatar_source(&a).await.unwrap().as_deref(), Some("one"));
        assert_eq!(db.avatar_source(&b).await.unwrap().as_deref(), Some("two"));
    }
}
