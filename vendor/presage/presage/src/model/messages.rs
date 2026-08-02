use libsignal_service::prelude::Content;
use libsignal_service::protocol::ServiceId;

#[derive(Debug)]
pub enum Received {
    /// when the receive loop is empty, happens when opening the websocket for the first time
    /// once you're done synchronizing all pending messages for this registered client.
    QueueEmpty,

    /// Got contacts (only applies if linked to a primary device
    /// Contacts can be later queried in the store.
    Contacts,

    /// Incoming decrypted message with metadata and content
    Content(Box<Content>),

    /// A message that could not be decrypted, and who sent it.
    ///
    /// The session with that device is no longer one both sides agree on, and
    /// nothing it sends from now on will decrypt either: the way out is to
    /// archive the session and let a new one be negotiated
    /// ([`Manager::send_session_reset`](crate::Manager::send_session_reset)),
    /// which is the caller's to decide, not least because it costs a message.
    Undecryptable(ServiceId),
}
