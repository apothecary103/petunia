use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use std::path::PathBuf;

use super::Command;
use petunia_data::attachment;
use petunia_data::{Connection, Contact, Fragment, Group, Message, Status, Thread};

#[derive(Debug, Clone)]
pub enum Event {
    Ready(UnboundedSender<Command>),
    LinkUrl(String),
    Linked {
        aci: Uuid,
    },
    Contacts {
        contacts: Vec<Contact>,
        groups: Vec<Group>,
    },
    /// A name from someone's profile. Signal's contact sync only carries names
    /// the user typed on their own phone, so for group members and anyone never
    /// saved as a contact this is the only place a name exists.
    Profile {
        uuid: Uuid,
        name: String,
    },
    /// A path rather than bytes: the view layer decodes and caches it off the
    /// UI thread, and the file survives a restart.
    Avatar {
        thread: Thread,
        path: PathBuf,
    },
    Attachment {
        thread: Thread,
        id: attachment::Id,
        blob: attachment::Blob,
        /// What an image turned out to be, when the sender did not say. Read
        /// from the file's header rather than by decoding it.
        measured: Option<attachment::Size>,
    },
    /// A thread has been read up to this point on another device, so its unread
    /// count here is stale.
    Read {
        thread: Thread,
        upto: u64,
    },
    /// How many messages a thread has that arrived after its read mark. Sent
    /// once at startup, so unread counts survive a restart.
    Unread {
        thread: Thread,
        count: u32,
    },
    /// What the list has been told about every conversation, at startup.
    Flags(Vec<(Thread, petunia_data::index::Flags)>),
    /// What a search turned up. Carries the query it answers, so a result that
    /// arrives after the box has moved on can be dropped rather than shown.
    Found {
        query: String,
        hits: Vec<crate::db::search::Hit>,
    },
    /// A still generated for a downloaded video.
    Poster {
        thread: Thread,
        id: attachment::Id,
        path: PathBuf,
    },
    /// Every installed sticker pack, with each sticker's bytes already written
    /// into the media cache.
    StickerPacks(Vec<petunia_data::stickers::Pack>),
    Preview {
        thread: Thread,
        message: Message,
    },
    History {
        thread: Thread,
        messages: Vec<Message>,
        /// Whether older messages remain behind this page.
        more: bool,
        /// Set when this page was requested by scrolling back rather than by
        /// opening the thread.
        older: bool,
    },
    /// A row as it arrived, still classified: a reaction or an edit is not a
    /// message but a change to one, and dropping that distinction is why they
    /// only used to appear after a reload.
    Fragment {
        thread: Thread,
        fragment: Fragment,
        /// When the row itself was sent, which orders competing edits.
        order: u64,
    },
    MessageStatus {
        timestamps: Vec<u64>,
        status: Status,
    },
    /// Someone is typing, or has stopped. Carries no timestamp: the UI expires
    /// the indicator itself, because a "stopped" can be lost.
    Typing {
        thread: Thread,
        sender: Uuid,
        started: bool,
    },
    /// Whether the message stream is up. The worker knows; nothing else can.
    Connection(Connection),
    Error(String),
}

