use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use super::Command;
use crate::data::{Contact, Group, Message, Status, Thread};

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
    Avatar {
        thread: Thread,
        bytes: Vec<u8>,
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
    Message {
        thread: Thread,
        message: Message,
    },
    MessageStatus {
        timestamps: Vec<u64>,
        status: Status,
    },
    Error(String),
}
