//! Opening the store, which is one encrypted SQLite file.
//!
//! Everything petunia knows is in it: the protocol sessions, the identity keys,
//! every message ever received and petunia's own tables beside them. Unencrypted
//! that is a plaintext archive of the account readable by anything that can read
//! the disk, which is what a backup, a stolen laptop or another application is.
//! SQLCipher is compiled in already -- presage-store-sqlite bundles it -- so the
//! only thing missing was a key, and `key::passphrase` keeps one in the
//! platform's own secret store.
//!
//! A file written by an earlier version has none, and that is not something to
//! leave to somebody to notice: `convert` migrates it the first time it is
//! opened, through SQLCipher's own `sqlcipher_export`, and leaves the plaintext
//! original beside it as a backup rather than deleting it.

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{ConnectOptions, Connection};

use super::{Error, key};
use petunia_config as config;
use presage_store_sqlite::{OnNewIdentity, SqliteStore};

/// What the plaintext file is renamed to once it has been converted. Kept rather
/// than deleted: this is the one moment the whole account passes through a
/// rewrite, and a backup costs a copy of a file already on this disk.
const BACKUP: &str = "store.plaintext.bak";

pub async fn open() -> Result<SqliteStore, Error> {
    let passphrase = ready().await?;

    Ok(SqliteStore::open_with_passphrase(
        &config::store_path().to_string_lossy(),
        Some(&passphrase),
        OnNewIdentity::Trust,
    )
    .await?)
}

/// The key the store is encrypted with, having made sure the file on disk really
/// is encrypted with it.
///
/// Everything that opens the store goes through this, presage's pool and
/// petunia's both -- and petunia's opens first. Left to `open` alone, a store
/// written before any of this was still plaintext when `db::Db` reached it with a
/// key in hand, which reads as a corrupt database rather than as a file that has
/// not been converted yet. Idempotent, so being called twice a launch costs one
/// keyring read and one schema query.
pub async fn ready() -> Result<String, Error> {
    let path = config::store_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let passphrase = key::passphrase()?;
    prepare(&path, &passphrase).await?;
    Ok(passphrase)
}

/// Leaves the file at `path` encrypted with `passphrase`, whatever it was
/// before. One already encrypted with it is left alone; one written before
/// petunia encrypted anything is converted; one that is neither is an error,
/// because opening it would create a second, empty store beside an account
/// nobody could reach again.
async fn prepare(path: &std::path::Path, passphrase: &str) -> Result<(), Error> {
    if !tokio::fs::try_exists(path).await? {
        return Ok(());
    }

    match state(path, passphrase).await {
        State::Encrypted => Ok(()),
        State::Plaintext => convert(path, passphrase).await,
        State::Unreadable => Err(Error::Encryption(format!(
            "{} cannot be read with this account's key. If the key in the system \
             keyring was replaced, the store can only be opened with the one it \
             was encrypted with; the account has to be linked again.",
            path.display()
        ))),
    }
}

enum State {
    Encrypted,
    Plaintext,
    Unreadable,
}

/// Which of the three it is, asked by trying to read the schema. There is no
/// other question to ask: an encrypted file opened without its key and a corrupt
/// one are the same thing to SQLite, and so are a plaintext file opened with a
/// key and a wrong key.
async fn state(path: &std::path::Path, passphrase: &str) -> State {
    if readable(options(path, Some(passphrase))).await {
        return State::Encrypted;
    }
    if readable(options(path, None)).await {
        return State::Plaintext;
    }
    State::Unreadable
}

async fn readable(options: SqliteConnectOptions) -> bool {
    let Ok(mut connection) = options.connect().await else {
        return false;
    };
    let readable = sqlx::query("SELECT count(*) FROM sqlite_master")
        .fetch_one(&mut connection)
        .await
        .is_ok();
    let _ = connection.close().await;
    readable
}

/// Rewrites a plaintext store as an encrypted one.
///
/// `sqlcipher_export` copies every page through a second, keyed database, which
/// is SQLCipher's own answer to this and the only one that carries the schema and
/// the indices across intact. The new file is built beside the old one and moved
/// into place, so an interruption leaves the original where it was rather than
/// half a store.
async fn convert(path: &std::path::Path, passphrase: &str) -> Result<(), Error> {
    let encrypted = path.with_extension("encrypting");
    let backup = path.with_file_name(BACKUP);
    // A leftover from a conversion that was interrupted: a partial copy of a
    // file that is still intact, so there is nothing in it worth keeping.
    let _ = tokio::fs::remove_file(&encrypted).await;

    tracing::info!("encrypting the store; the plaintext copy is kept as a backup");

    // `create_if_missing` for the sake of the attached file, not this one, which
    // the caller has already found: `ATTACH` opens with the flags the connection
    // was opened with, and without it cannot create the database it exports to.
    let mut connection = options(path, None).create_if_missing(true).connect().await?;
    // The write-ahead log folded back into the file first: the export reads the
    // database, and what is only in the log is not in it yet.
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&mut connection)
        .await?;
    // `ATTACH` takes no bind parameters -- neither does `PRAGMA key` -- so both
    // values are quoted into the statement, which is what `escaped` is for.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ATTACH DATABASE '{}' AS encrypted KEY '{}'",
        escaped(&encrypted.to_string_lossy()),
        escaped(passphrase),
    )))
    .execute(&mut connection)
    .await?;
    sqlx::query("SELECT sqlcipher_export('encrypted')")
        .fetch_all(&mut connection)
        .await?;
    sqlx::query("DETACH DATABASE encrypted")
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    // Only once the export has closed cleanly, and in this order, so there is
    // never a moment with no readable store on disk.
    tokio::fs::rename(path, &backup).await?;
    tokio::fs::rename(&encrypted, path).await?;
    // The old journal describes the file that is now the backup; left beside the
    // new one, SQLite would try to recover the wrong database out of it.
    for suffix in ["-wal", "-shm"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        let _ = tokio::fs::remove_file(std::path::PathBuf::from(companion)).await;
    }

    tracing::info!(backup = %backup.display(), "the store is encrypted");
    Ok(())
}

/// How every connection to the store is opened, keyed or not. One function, so
/// petunia's own pool and presage's cannot disagree about the key -- they open
/// the same file, and a pool that forgets the key sees a corrupt database.
pub fn options(path: &std::path::Path, passphrase: Option<&str>) -> SqliteConnectOptions {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full);

    match passphrase {
        Some(passphrase) => options.pragma("key", format!("'{}'", escaped(passphrase))),
        None => options,
    }
}

/// A single quote inside a SQL string literal, which is where both the key and
/// the attached path end up. The key is hex and a path rarely has one -- but
/// "rarely" in a string concatenated into a statement is the shape of every
/// injection there has ever been.
fn escaped(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    /// A plaintext database with something recognisable in it, the way a store
    /// written before any of this looks.
    async fn plaintext(path: &std::path::Path) {
        let mut connection = options(path, None)
            .create_if_missing(true)
            .connect()
            .await
            .unwrap();
        sqlx::query("CREATE TABLE kv (key TEXT PRIMARY KEY, value BLOB NOT NULL)")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO kv VALUES ('registration', x'0102')")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    }

    async fn registration(path: &std::path::Path, passphrase: Option<&str>) -> Option<Vec<u8>> {
        let mut connection = options(path, passphrase).connect().await.ok()?;
        let value = sqlx::query("SELECT value FROM kv WHERE key = 'registration'")
            .fetch_one(&mut connection)
            .await
            .ok()
            .map(|row| row.get::<Vec<u8>, _>(0));
        let _ = connection.close().await;
        value
    }

    #[tokio::test]
    async fn a_store_that_does_not_exist_yet_needs_no_preparing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db3");

        prepare(&path, "abcd").await.unwrap();

        assert!(!path.exists());
    }

    /// The one that matters: an account written before petunia encrypted
    /// anything has to still be there afterwards.
    #[tokio::test]
    async fn a_plaintext_store_is_converted_and_keeps_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db3");
        plaintext(&path).await;

        prepare(&path, "abcd").await.unwrap();

        assert_eq!(registration(&path, Some("abcd")).await, Some(vec![1, 2]));
    }

    #[tokio::test]
    async fn a_converted_store_can_no_longer_be_read_without_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db3");
        plaintext(&path).await;

        prepare(&path, "abcd").await.unwrap();

        assert_eq!(registration(&path, None).await, None);
    }

    #[tokio::test]
    async fn converting_keeps_the_plaintext_original_as_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db3");
        plaintext(&path).await;

        prepare(&path, "abcd").await.unwrap();

        assert_eq!(
            registration(&dir.path().join(BACKUP), None).await,
            Some(vec![1, 2])
        );
    }

    /// Every launch after the first goes through this, so it has to be a no-op
    /// rather than a second conversion.
    #[tokio::test]
    async fn an_encrypted_store_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db3");
        plaintext(&path).await;
        prepare(&path, "abcd").await.unwrap();

        prepare(&path, "abcd").await.unwrap();

        assert_eq!(registration(&path, Some("abcd")).await, Some(vec![1, 2]));
        // Converted once, so the backup is still the plaintext original rather
        // than the encrypted file a second pass would have moved aside.
        assert_eq!(
            registration(&dir.path().join(BACKUP), None).await,
            Some(vec![1, 2])
        );
    }

    /// Better to refuse than to open a fresh empty store beside an account
    /// nobody can reach any more.
    #[tokio::test]
    async fn a_store_the_key_does_not_open_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db3");
        plaintext(&path).await;
        prepare(&path, "abcd").await.unwrap();

        let refused = prepare(&path, "dcba").await;

        assert!(matches!(refused, Err(Error::Encryption(_))));
    }

    #[test]
    fn a_quote_in_a_key_is_escaped_rather_than_ending_the_literal() {
        assert_eq!(escaped("it's"), "it''s");
        assert_eq!(escaped("plain"), "plain");
    }
}
