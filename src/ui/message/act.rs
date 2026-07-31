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

use crate::data::MessageId;
use crate::data::attachment;

#[derive(Debug, Clone)]
pub enum Act {
    /// Fetch an attachment the auto-download policy skipped.
    Download {
        timestamp: u64,
        id: attachment::Id,
    },
    Reply(MessageId),
    React(MessageId, String),
    Edit(MessageId),
    Delete(MessageId),
    /// Copies the message's text to the clipboard.
    Copy(MessageId),
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
    /// Installs the pack a received sticker came from.
    InstallStickers { pack_id: Vec<u8>, key: Vec<u8> },
    /// Opens a link in the browser.
    OpenLink(String),
    /// Opens someone's profile in the details panel.
    Inspect(uuid::Uuid),
}

pub type Dispatch = Rc<dyn Fn(Act, &mut Window, &mut App)>;
