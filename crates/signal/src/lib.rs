mod cache;
pub mod command;
pub mod db;
mod event;
mod key;
mod outgoing;
mod store;
mod worker;

pub use command::Command;
pub use event::Event;

/// Starts the engine on a thread of its own and streams everything it learns
/// down `events`.
///
/// The thread is spawned here rather than by the caller because of the stack:
/// presage's receive futures are huge -- multi-megabyte in debug builds -- and
/// the default 2 MiB overflows while polling them.
pub fn spawn(events: futures::channel::mpsc::Sender<Event>) {
    std::thread::Builder::new()
        .name("signal".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || worker::run(events))
        .expect("spawn signal worker thread");
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(#[from] presage_store_sqlite::SqliteStoreError),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("signal error: {0}")]
    Signal(#[from] presage::Error<presage_store_sqlite::SqliteStoreError>),
    #[error("could not reach the system keyring for the store's key: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("{0}")]
    Encryption(String),
}
