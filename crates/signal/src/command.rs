use std::path::PathBuf;

use petunia_data::attachment;
use petunia_data::message::Range;
use petunia_data::{MessageId, Thread};

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
    /// Withdraws a message from everybody who received it, leaving a tombstone
    /// in its place. Only ever our own: Signal honours a remote delete from the
    /// message's own author and from nobody else.
    DeleteMessage {
        thread: Thread,
        target: u64,
        timestamp: u64,
    },
    /// Drops a message from this account without asking anybody else to. Works
    /// on somebody else's message, which is the whole reason it exists, and
    /// syncs to the account's other devices.
    DeleteForMe {
        thread: Thread,
        target: MessageId,
    },
    /// Blocks or unblocks somebody, through the `blocked` flag on their Storage
    /// Service contact record -- which is where Signal's block list lives, so
    /// this is what every linked device reads.
    SetBlocked {
        contact: uuid::Uuid,
        blocked: bool,
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
        flags: petunia_data::index::Flags,
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
    /// Reads a pack this account does not have, so it can be shown before it is
    /// added: the manifest lives behind the pack key and fetching it is the only
    /// way to know what a pack is called or what else is in it.
    PreviewStickerPack {
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
    /// Sets or clears the nickname and note shown for a contact, synced to
    /// every device linked to this account through Signal's Storage Service.
    SetNickname {
        contact: uuid::Uuid,
        name: Option<String>,
        note: Option<String>,
    },
    /// Replaces this account's profile picture, from the bytes of an image
    /// already read off disk -- the worker only ever reads them again to
    /// upload them, so the UI does the one read and hands them over.
    SetAvatar {
        bytes: Vec<u8>,
    },
    SendPoll {
        thread: Thread,
        question: String,
        options: Vec<String>,
        allow_multiple: bool,
        timestamp: u64,
    },
    VotePoll {
        thread: Thread,
        target: MessageId,
        option_indexes: Vec<u32>,
        count: u32,
        timestamp: u64,
    },
    TerminatePoll {
        thread: Thread,
        target: u64,
        timestamp: u64,
    },
    /// Unlinks this device from Signal's servers, then clears everything
    /// stored locally. There is no undo -- the account has to be linked again
    /// from a fresh QR code afterward.
    LogOut,
    /// Reserves and confirms a username, then publishes the sharable link.
    /// Replaces whatever username this account already had. `nickname` may
    /// carry a chosen discriminator , in which case that exact
    /// username is asked for rather than one Signal picks a number for.
    SetUsername {
        nickname: String,
    },
    DeleteUsername,
    /// Resolves what was typed into the new-chat field -- a username, or a
    /// phone number -- to an account, so a conversation can be opened with
    /// somebody who is not in the contact list.
    LookUp {
        query: String,
    },
    /// Creates a group with `members` in it and tells them about it. The
    /// creator is an administrator; everybody else joins as a member, or as an
    /// invitation where their profile key is not known.
    CreateGroup {
        title: String,
        members: Vec<uuid::Uuid>,
        timestamp: u64,
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
