#[cfg(test)]
use sqlx::Row;

use super::Db;
use petunia_data::attachment::Id;
use crate::Error;

impl Db {
    pub async fn record_blob(
        &self,
        id: &Id,
        content_type: &str,
        bytes: u64,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO petunia_blob (digest, content_type, bytes, fetched_at)
            VALUES (?, ?, ?, unixepoch())
            ON CONFLICT DO UPDATE SET fetched_at = unixepoch()",
        )
        .bind(id.as_str())
        .bind(content_type)
        .bind(bytes as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Called after a prune, so the table never claims to know about bytes that
    /// are no longer on disk.
    pub async fn forget_blobs(&self, ids: &[Id]) -> Result<(), Error> {
        for id in ids {
            sqlx::query("DELETE FROM petunia_blob WHERE digest = ?")
                .bind(id.as_str())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub async fn cached_bytes(&self) -> Result<u64, Error> {
        let total: i64 = sqlx::query("SELECT COALESCE(SUM(bytes), 0) FROM petunia_blob")
            .fetch_one(&self.pool)
            .await?
            .get(0);
        Ok(total as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_and_totals_cached_blobs() {
        let db = Db::open_in_memory().await.unwrap();
        assert_eq!(db.cached_bytes().await.unwrap(), 0);

        db.record_blob(&Id::from_hex("aa"), "image/png", 100)
            .await
            .unwrap();
        db.record_blob(&Id::from_hex("bb"), "image/png", 250)
            .await
            .unwrap();

        assert_eq!(db.cached_bytes().await.unwrap(), 350);
    }

    #[tokio::test]
    async fn forgetting_pruned_blobs_stops_counting_them() {
        let db = Db::open_in_memory().await.unwrap();
        db.record_blob(&Id::from_hex("aa"), "image/png", 100)
            .await
            .unwrap();
        db.record_blob(&Id::from_hex("bb"), "image/png", 250)
            .await
            .unwrap();

        db.forget_blobs(&[Id::from_hex("aa")]).await.unwrap();

        assert_eq!(db.cached_bytes().await.unwrap(), 250);
    }

    #[tokio::test]
    async fn re_recording_the_same_digest_does_not_double_count() {
        let db = Db::open_in_memory().await.unwrap();

        db.record_blob(&Id::from_hex("aa"), "image/png", 100)
            .await
            .unwrap();
        db.record_blob(&Id::from_hex("aa"), "image/png", 100)
            .await
            .unwrap();

        assert_eq!(db.cached_bytes().await.unwrap(), 100);
    }
}
