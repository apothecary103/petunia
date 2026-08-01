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
        /// In Signal's own +E.164 shape, for display -- nothing here dials it.
        phone_number: String,
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
    StickerPacks {
        packs: Vec<petunia_data::stickers::Pack>,
        /// Installed packs that nothing could be drawn of, because none of their
        /// stickers ever arrived. Counted rather than dropped silently: an empty
        /// picker saying "no sticker packs yet" to somebody who has ten of them
        /// is the failure reporting itself as a fresh install.
        unreadable: usize,
    },
    /// A pack that was read but not installed, for the sheet that offers to add
    /// it. `Err` carries what to say when the fetch failed, since a sheet that
    /// waits forever is the same as one that lies.
    StickerPackPreview {
        pack_id: Vec<u8>,
        pack: Result<petunia_data::stickers::Pack, String>,
    },
    /// What the sidebar draws for a thread's newest message: the line, not the
    /// message. Read back off disk at startup, so the list is whole before a
    /// single row has been decoded or a single profile fetched.
    Preview {
        thread: Thread,
        preview: petunia_data::index::Preview,
    },
    /// That a thread has something in it, with no line to show for it -- its
    /// newest rows are reactions, edits and tombstones, which project to nothing
    /// renderable. Enough to keep it listed, which is what it was not.
    Activity {
        thread: Thread,
        at: u64,
    },
    History {
        thread: Thread,
        messages: Vec<Message>,
        /// Whether older messages remain behind this page.
        more: bool,
        /// The oldest stored row this page reached, which is what the page behind
        /// it must ask from. Not the oldest message: a reaction or an edit is a
        /// row that adds no message, and a page of only those would otherwise
        /// leave the reader asking for it again forever.
        covered: Option<u64>,
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
    /// This device has been unlinked and its local store cleared. Nothing but
    /// linking again can follow.
    LoggedOut,
    /// The account's username, and the `signal.me` link it confirmed with.
    /// `None` when it was deleted rather than set.
    Username(Option<(String, String)>),
    /// What a new-chat lookup resolved to. Carries the query it answers, so an
    /// answer that arrives after the field has moved on can be dropped.
    LookedUp {
        query: String,
        found: Option<Contact>,
    },
    /// A group this account has just made, ready to be opened.
    GroupCreated {
        thread: Thread,
    },
    /// A nickname and note for a contact, from Storage Service or from this
    /// account just having set one. `None` for either half means it was
    /// cleared, not that it was left unread.
    Nickname {
        uuid: Uuid,
        name: Option<String>,
        note: Option<String>,
    },
    /// Somebody's block state, read out of Storage Service at startup or set
    /// here just now.
    Blocked {
        uuid: Uuid,
        blocked: bool,
    },
    /// A message this device is to forget, because "delete for me" happened --
    /// here, or on another device that told us about it.
    Forgotten {
        thread: Thread,
        target: petunia_data::MessageId,
    },
    /// Our own avatar changed, at the path the worker cached the new picture
    /// under -- the same shape `Event::Avatar` already uses for everyone
    /// else's.
    AvatarUpdated(std::path::PathBuf),
}

