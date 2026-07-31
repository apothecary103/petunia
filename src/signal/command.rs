use std::path::PathBuf;

use crate::data::attachment;
use crate::data::message::Range;
use crate::data::{MessageId, Thread};

/// How many renderable messages a page aims to produce.
pub const PAGE: u32 = 50;

/// What a reply needs to know about the message it answers.
#[derive(Debug, Clone)]
pub struct Quoted {
    pub id: MessageId,
    pub body: String,
    pub ranges: Vec<Range>,
}

#[derive(Debug, Clone)]
pub enum Command {
    SendText {
        thread: Thread,
        body: String,
        /// Signal carries formatting as offsets over the body, not as markup.
        ranges: Vec<Range>,
        /// Set when this is a reply; carries a snapshot of the quoted message
        /// because the recipient may not have the original.
        quote: Option<Quoted>,
        timestamp: u64,
    },
    React {
        thread: Thread,
        target: MessageId,
        emoji: String,
        remove: bool,
        timestamp: u64,
    },
    /// Only ever our own messages: Signal has no way to delete someone else's.
    DeleteMessage {
        thread: Thread,
        target: u64,
        timestamp: u64,
    },
    EditMessage {
        thread: Thread,
        target: u64,
        body: String,
        ranges: Vec<Range>,
        timestamp: u64,
    },
    /// Uploads the files, then sends them with an optional caption. Paths rather
    /// than bytes: the worker reads them, so the UI never holds an attachment.
    SendAttachments {
        thread: Thread,
        body: String,
        ranges: Vec<Range>,
        paths: Vec<PathBuf>,
        quote: Option<Quoted>,
        timestamp: u64,
    },
    LoadThread {
        thread: Thread,
        /// Loads the messages immediately older than this timestamp; `None`
        /// loads the newest page.
        before: Option<u64>,
    },
    /// Marks everything up to and including `upto` read: sends READ receipts to
    /// the senders and a read sync to our own other devices.
    MarkRead {
        thread: Thread,
        /// Whose message, and when, for each receipt that is owed.
        messages: Vec<(uuid::Uuid, u64)>,
    },
    Typing {
        thread: Thread,
        started: bool,
    },
    /// Sends a sticker from an installed pack. The bytes are re-uploaded because
    /// presage keeps the decrypted image and drops the pointer it arrived under.
    SendSticker {
        thread: Thread,
        pack_id: Vec<u8>,
        key: Vec<u8>,
        sticker_id: u32,
        emoji: Option<String>,
        path: PathBuf,
        timestamp: u64,
    },
    /// Records what the list has been told about a conversation: pinned,
    /// archived, muted, foldered. Local to this device.
    SetFlags {
        thread: Thread,
        flags: crate::data::index::Flags,
    },
    /// Forgets a conversation: its messages and everything recorded about it.
    /// Local to this device, like the flags and for the same reason -- Signal's
    /// own "delete for me" goes through the Storage Service, which presage does
    /// not expose.
    DeleteThread {
        thread: Thread,
    },
    /// Looks for a phrase, in one conversation or in all of them.
    Search {
        query: String,
        within: Option<Thread>,
    },
    /// Installs the pack a received sticker came from, and tells our other
    /// devices. The key only ever travels alongside the sticker itself.
    InstallStickerPack {
        pack_id: Vec<u8>,
        key: Vec<u8>,
    },
    /// Fetches an attachment the auto-download policy skipped. The pointer is
    /// re-read from the stored row rather than carried through the UI.
    DownloadAttachment {
        thread: Thread,
        timestamp: u64,
        id: attachment::Id,
    },
}

impl Command {
    pub fn load(thread: Thread) -> Self {
        Self::LoadThread {
            thread,
            before: None,
        }
    }
}
