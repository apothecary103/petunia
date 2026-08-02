//! Disappearing messages: what each conversation is set to, and what is due.
//!
//! Signal gives a one-to-one timer no home a client can read back — it arrives
//! as an `EXPIRATION_TIMER_UPDATE` and is thereafter the client's to remember —
//! so it is cached here and read at startup, the way the previews and the names
//! are. A group carries its own in its record and seeds this.
//!
//! When the clock starts is Signal's rule: the moment a message is sent, for
//! our own, and the moment it is read, for everybody else's. So a deadline is
//! written at that moment rather than when the message arrives, and a message
//! nobody has read yet has no row here at all.

use sqlx::Row;

use super::Db;
use petunia_data::{MessageId, Thread};
use crate::Error;

impl Db {
    /// Records what a conversation is set to. Zero is off, and off is the
    /// absence of a row rather than a row saying nothing.
    pub async fn set_expire_timer(&self, thread: &Thread, seconds: u32) -> Result<(), Error> {
        if seconds == 0 {
            sqlx::query("DELETE FROM petunia_expire_timers WHERE thread = ?")
                .bind(super::read::key(thread))
                .execute(&self.pool)
                .await?;
            return Ok(());
        }

        sqlx::query(
            "INSERT INTO petunia_expire_timers (thread, is_group, seconds)
            VALUES (?, ?, ?)
            ON CONFLICT DO UPDATE SET seconds = excluded.seconds",
        )
        .bind(super::read::key(thread))
        .bind(matches!(thread, Thread::Group(_)))
        .bind(i64::from(seconds))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every conversation with a timer on it, for the window to start from.
    pub async fn expire_timers(&self) -> Result<Vec<(Thread, u32)>, Error> {
        let rows = sqlx::query("SELECT thread, is_group, seconds FROM petunia_expire_timers")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .iter()
            .filter_map(|row| {
                let thread = super::read::thread(row.get(0), row.get(1))?;
                Some((thread, row.get::<i64, _>(2).max(0) as u32))
            })
            .collect())
    }

    /// Starts a message's clock, if it has not been started already. Ignored
    /// on conflict rather than overwritten: reading a conversation twice must
    /// not give its messages a fresh hour each time.
    pub async fn start_expiry(
        &self,
        thread: &Thread,
        target: MessageId,
        expires_at: u64,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_expiring (thread, is_group, ts, sender, expires_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT DO NOTHING",
        )
        .bind(super::read::key(thread))
        .bind(matches!(thread, Thread::Group(_)))
        .bind(target.timestamp as i64)
        .bind(target.sender.as_bytes().to_vec())
        .bind(expires_at as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Everything whose time is up.
    pub async fn due(&self, now: u64) -> Result<Vec<(Thread, MessageId)>, Error> {
        let rows = sqlx::query(
            "SELECT thread, is_group, ts, sender FROM petunia_expiring WHERE expires_at <= ?",
        )
        .bind(now as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .filter_map(|row| {
                let thread = super::read::thread(row.get(0), row.get(1))?;
                let sender = uuid::Uuid::from_slice(&row.get::<Vec<u8>, _>(3)).ok()?;
                Some((
                    thread,
                    MessageId {
                        timestamp: row.get::<i64, _>(2) as u64,
                        sender,
                    },
                ))
            })
            .collect())
    }

    /// Forgets a deadline, whether it was met or the message went first.
    pub async fn forget_expiry(&self, thread: &Thread, timestamp: u64) -> Result<(), Error> {
        sqlx::query("DELETE FROM petunia_expiring WHERE thread = ? AND ts = ?")
            .bind(super::read::key(thread))
            .bind(timestamp as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use petunia_data::ContactId;
    use uuid::Uuid;

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    fn message(timestamp: u64) -> MessageId {
        MessageId {
            timestamp,
            sender: Uuid::new_v4(),
        }
    }

    #[tokio::test]
    async fn nothing_disappears_to_begin_with() {
        let db = Db::open_in_memory().await.unwrap();

        assert!(db.expire_timers().await.unwrap().is_empty());
        assert!(db.due(u64::MAX).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_timer_round_trips() {
        let db = Db::open_in_memory().await.unwrap();
        let (contact, group) = (thread(), Thread::Group([7; 32]));

        db.set_expire_timer(&contact, 3_600).await.unwrap();
        db.set_expire_timer(&group, 30).await.unwrap();

        let mut timers = db.expire_timers().await.unwrap();
        timers.sort_by_key(|(_, seconds)| *seconds);
        assert_eq!(timers, [(group, 30), (contact, 3_600)]);
    }

    /// Off is the absence of a row, not a row saying nothing: otherwise the
    /// table fills with conversations that had a timer once.
    #[tokio::test]
    async fn turning_a_timer_off_forgets_it() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();

        db.set_expire_timer(&thread, 3_600).await.unwrap();
        db.set_expire_timer(&thread, 0).await.unwrap();

        assert!(db.expire_timers().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_what_is_due_comes_back() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let (soon, later) = (message(1), message(2));

        db.start_expiry(&thread, soon, 1_000).await.unwrap();
        db.start_expiry(&thread, later, 9_000).await.unwrap();

        assert_eq!(db.due(5_000).await.unwrap(), [(thread.clone(), soon)]);
        assert_eq!(db.due(9_000).await.unwrap().len(), 2);
    }

    /// Reading a conversation twice must not grant its messages another hour
    /// each time, which is the whole reason the insert ignores a conflict.
    #[tokio::test]
    async fn a_clock_that_has_started_is_not_restarted() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let target = message(1);

        db.start_expiry(&thread, target, 1_000).await.unwrap();
        db.start_expiry(&thread, target, 500_000).await.unwrap();

        assert_eq!(db.due(2_000).await.unwrap(), [(thread, target)]);
    }

    #[tokio::test]
    async fn a_deadline_can_be_forgotten() {
        let db = Db::open_in_memory().await.unwrap();
        let thread = thread();
        let target = message(1);

        db.start_expiry(&thread, target, 1_000).await.unwrap();
        db.forget_expiry(&thread, target.timestamp).await.unwrap();

        assert!(db.due(u64::MAX).await.unwrap().is_empty());
    }
}
