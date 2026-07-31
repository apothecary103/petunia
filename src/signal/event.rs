use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use std::path::PathBuf;

use super::Command;
use crate::data::attachment;
use crate::data::{Contact, Fragment, Group, Message, Status, Thread};

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
    /// A path rather than bytes: iced hashes it and loads off the UI thread in
    /// its own image pipeline, and the file survives a restart.
    Avatar {
        thread: Thread,
        path: PathBuf,
    },
    Attachment {
        thread: Thread,
        id: attachment::Id,
        blob: attachment::Blob,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Connection {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
}

impl Connection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting…",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting…",
        }
    }
}
