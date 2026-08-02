//! One protocol-store writer at a time.
//!
//! Encrypting and decrypting are both a read-modify-write of the same session
//! record: libsignal loads it, ratchets it, and stores it back, with the store's
//! own awaits in between. Two of those in flight at once — a receipt going out
//! while the stream is opening an envelope from the same device — is a lost
//! update, and the one that loses is rolled back to the state it was loaded
//! from.
//!
//! The Double Ratchet survives that, since a stale root key still derives the
//! chain the sender is using. The post-quantum ratchet does not: its state is
//! sparse and ordered, and a rollback is unrecoverable. Every later message from
//! that device then fails with `post-quantum ratchet error`, forever, which is
//! what a permanently broken session with one's own phone looks like.
//!
//! So the crypto is serialised — the network is not. Only the encrypt and
//! decrypt themselves are held, not the round trip that carries the result.

use futures::lock::{Mutex, MutexGuard};
use std::sync::LazyLock;

static SESSIONS: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Held for the whole of one read-modify-write of the session records.
pub async fn sessions() -> MutexGuard<'static, ()> {
    SESSIONS.lock().await
}
