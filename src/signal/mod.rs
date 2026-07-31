pub mod bridge;
mod cache;
mod command;
mod db;
mod event;
mod outgoing;
mod store;
mod worker;

pub use command::{Command, Quoted};
pub use event::{Connection, Event};

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
}
