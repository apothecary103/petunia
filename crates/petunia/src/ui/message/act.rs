//! What a message can be asked to do, and the one way of asking.
//!
//! Every control drawn on a message — the hover bar, a reaction chip, an
//! attachment, a sticker — reports through a single closure rather than through
//! a field of its own. Adding a control means adding a variant here and an arm
//! where the conversation handles it, not another `Rc` threaded through four
//! call sites.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Window};

use petunia_data::MessageId;
use petunia_data::attachment;

#[derive(Debug, Clone)]
pub enum Act {
    /// Fetch an attachment the auto-download policy skipped.
    Download {
        timestamp: u64,
        id: attachment::Id,
    },
    Reply(MessageId),
    React(MessageId, String),
    /// Toggles an option on a poll you have not already voted the same way
    /// on. Carries every option that should be checked afterward, since
    /// Signal's own vote is a full ballot rather than a single toggle.
    VotePoll(MessageId, Vec<u32>),
    TerminatePoll(MessageId),
    Edit(MessageId),
    /// Asks which deletion. Never deletes anything itself: there are two and
    /// they mean different things, so the choice is the reader's.
    Delete(MessageId),
    /// Copies the message's text to the clipboard.
    Copy(MessageId),
    /// Sends this message on to another conversation.
    Forward(MessageId),
    /// Shows what the wire actually said about this message.
    Raw(MessageId),
    /// Opens a picture full size, with everything else in the thread beside it.
    View(PathBuf),
    /// Writes a copy somewhere the user picks.
    Save(PathBuf),
    /// Hands a file to whatever the system opens it with.
    Open(PathBuf),
    /// Plays a voice note or an audio file, or pauses it if it is already
    /// playing.
    Play(PathBuf),
    /// Jumps to a fraction of the way through what is playing.
    Seek(PathBuf, f32),
    /// Opens the sticker, and the pack it came from. Boxed because a sticker
    /// carries an attachment and every other variant here is a word wide.
    ShowSticker(Box<petunia_data::message::Sticker>),
    /// Opens a link in the browser.
    OpenLink(String),
    /// Opens someone's profile in the details panel.
    Inspect(uuid::Uuid),
    /// Asks for a nickname for somebody, which syncs to every device on this
    /// account.
    Nickname(uuid::Uuid),
    /// Blocks or unblocks somebody. Blocking asks first; unblocking does not,
    /// because it is the undo.
    Block(uuid::Uuid, bool),
    /// A right-click on a message, at this point on screen.
    Menu(MessageId, gpui::Point<gpui::Pixels>),
    /// A right-click on someone's name or picture.
    MenuFor(uuid::Uuid, gpui::Point<gpui::Pixels>),
}

pub type Dispatch = Rc<dyn Fn(Act, &mut Window, &mut App)>;
