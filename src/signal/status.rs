use std::collections::HashMap;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool};
use sqlx::{AssertSqlSafe, Row};

use super::Error;
use crate::config;
use crate::data::Status;

#[derive(Debug, Clone)]
pub struct StatusStore {
    db: SqlitePool,
}

// The table lives in presage's database file, so it is prefixed to stay out
// of the way of presage's own migrations.
impl StatusStore {
    pub async fn open() -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(config::store_path())
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let db = SqlitePool::connect_with(options).await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS petunia_message_status (
                timestamp INTEGER PRIMARY KEY,
                status INTEGER NOT NULL
            )",
        )
        .execute(&db)
        .await?;
        Ok(Self { db })
    }

    pub async fn set(&self, timestamp: u64, status: Status) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_message_status (timestamp, status) VALUES (?, ?)
            ON CONFLICT DO UPDATE SET status = excluded.status",
        )
        .bind(timestamp as i64)
        .bind(to_int(status))
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn upgrade(&self, timestamps: &[u64], status: Status) -> Result<(), Error> {
        for &timestamp in timestamps {
            sqlx::query(
                "INSERT INTO petunia_message_status (timestamp, status) VALUES (?, ?)
                ON CONFLICT DO UPDATE SET status = MAX(status, excluded.status)",
            )
            .bind(timestamp as i64)
            .bind(to_int(status))
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    pub async fn get(&self, timestamps: &[u64]) -> Result<HashMap<u64, Status>, Error> {
        let mut statuses = HashMap::new();
        for chunk in timestamps.chunks(500) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT timestamp, status FROM petunia_message_status
                WHERE timestamp IN ({placeholders})"
            );
            let mut query = sqlx::query(AssertSqlSafe(sql));
            for &timestamp in chunk {
                query = query.bind(timestamp as i64);
            }
            for row in query.fetch_all(&self.db).await? {
                if let Some(status) = from_int(row.get(1)) {
                    statuses.insert(row.get::<i64, _>(0) as u64, status);
                }
            }
        }
        Ok(statuses)
    }
}

fn to_int(status: Status) -> i64 {
    match status {
        Status::Sending => 0,
        Status::Failed => 1,
        Status::Sent => 2,
        Status::Delivered => 3,
        Status::Read => 4,
        Status::Viewed => 5,
    }
}

fn from_int(value: i64) -> Option<Status> {
    match value {
        0 => Some(Status::Sending),
        1 => Some(Status::Failed),
        2 => Some(Status::Sent),
        3 => Some(Status::Delivered),
        4 => Some(Status::Read),
        5 => Some(Status::Viewed),
        _ => None,
    }
}
