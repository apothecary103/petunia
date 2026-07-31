use presage_store_sqlite::{OnNewIdentity, SqliteStore};

use super::Error;
use petunia_config as config;

pub async fn open() -> Result<SqliteStore, Error> {
    let path = config::store_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(SqliteStore::open(&path.to_string_lossy(), OnNewIdentity::Trust).await?)
}
