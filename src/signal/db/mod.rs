pub mod avatars;
pub mod blobs;
pub mod messages;
pub mod read;
pub mod receipts;
pub mod search;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool};
use sqlx::Row;

use super::Error;
use crate::config;

/// Petunia's own tables live in presage's database file, so everything is
/// prefixed and no presage table ever gains a column.
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

/// Appended to, never edited: each entry is one schema version.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_receipts.sql"),
    include_str!("migrations/002_blobs.sql"),
    include_str!("migrations/003_read.sql"),
    include_str!("migrations/004_avatars.sql"),
    include_str!("migrations/005_search.sql"),
];

#[cfg(test)]
fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

impl Db {
    pub async fn open() -> Result<Self, Error> {
        Self::open_at(config::store_path()).await
    }

    async fn open_at(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePool::connect_with(options).await?;
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<(), Error> {
        self.migrate_upto(MIGRATIONS.len()).await
    }

    /// Bounded so a test can build a database at an older version and then
    /// migrate it forward, the way an existing install does.
    async fn migrate_upto(&self, upto: usize) -> Result<(), Error> {
        sqlx::query("CREATE TABLE IF NOT EXISTS petunia_schema (version INTEGER NOT NULL)")
            .execute(&self.pool)
            .await?;

        let applied: i64 = sqlx::query("SELECT version FROM petunia_schema")
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.get(0))
            .unwrap_or_default();

        for (index, step) in MIGRATIONS[..upto].iter().enumerate().skip(applied as usize) {
            let version = index as i64 + 1;
            let mut tx = self.pool.begin().await?;
            sqlx::raw_sql(*step).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM petunia_schema")
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO petunia_schema (version) VALUES (?)")
                .bind(version)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            tracing::info!(version, "applied petunia migration");
        }

        Ok(())
    }
}

#[cfg(test)]
impl Db {
    /// Stands in for the tables presage owns, which exist before petunia's
    /// migration runs because the store is opened first.
    pub async fn open_in_memory_unmigrated() -> Result<Self, Error> {
        let pool = SqlitePool::connect("sqlite::memory:").await?;
        sqlx::raw_sql(
            "CREATE TABLE threads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                group_master_key BLOB UNIQUE,
                recipient_id TEXT UNIQUE
            );
            CREATE TABLE thread_messages (
                ts                     INTEGER NOT NULL,
                thread_id              INTEGER NOT NULL,
                sender_service_id      TEXT    NOT NULL,
                sender_device_id       INTEGER NOT NULL,
                destination_service_id TEXT    NOT NULL,
                needs_receipt          BOOLEAN NOT NULL,
                unidentified_sender    BOOLEAN NOT NULL,
                content_body           BLOB    NOT NULL,
                was_plaintext          BOOLEAN NOT NULL,
                server_ts              INTEGER,
                PRIMARY KEY (ts, thread_id)
            );
            CREATE TABLE petunia_message_status (
                timestamp INTEGER PRIMARY KEY,
                status    INTEGER NOT NULL
            );
            CREATE TABLE profiles (key BLOB PRIMARY KEY, body BLOB NOT NULL);
            CREATE TABLE profile_avatars (key BLOB PRIMARY KEY, body BLOB NOT NULL);",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub async fn open_in_memory() -> Result<Self, Error> {
        let db = Self::open_in_memory_unmigrated().await?;
        db.migrate().await?;
        Ok(db)
    }

    pub async fn seed_legacy_status(&self, timestamp: u64, status: i64) -> Result<(), Error> {
        sqlx::query("INSERT INTO petunia_message_status (timestamp, status) VALUES (?, ?)")
            .bind(timestamp as i64)
            .bind(status)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn version(&self) -> Result<i64, Error> {
        Ok(sqlx::query("SELECT version FROM petunia_schema")
            .fetch_one(&self.pool)
            .await?
            .get(0))
    }

    pub async fn table_exists(&self, name: &str) -> Result<bool, Error> {
        Ok(
            sqlx::query("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?
                .is_some(),
        )
    }
}
