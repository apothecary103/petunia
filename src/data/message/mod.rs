pub mod project;
pub mod range;

use uuid::Uuid;

pub use project::{from_content, project, receipt_from_content};
pub use range::Range;

use super::attachment::Attachment;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId {
    pub timestamp: u64,
    pub sender: Uuid,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: MessageId,
    pub content: Content,
    pub attachments: Vec<Attachment>,
    pub quote: Option<Quote>,
    pub preview: Option<LinkPreview>,
    pub reactions: Vec<Reaction>,
    pub status: Option<Status>,
    pub edited: Option<u64>,
    pub expires_in: Option<u32>,
    pub view_once: bool,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text { body: String, ranges: Vec<Range> },
    Sticker(Sticker),
    Deleted,
    Update(Update),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    ExpireTimer { seconds: u32 },
    IdentityChanged,
    Group(String),
}

#[derive(Debug, Clone)]
pub struct Quote {
    pub id: MessageId,
    pub body: String,
    pub ranges: Vec<Range>,
    pub thumbnail: Option<Attachment>,
}

#[derive(Debug, Clone)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<Attachment>,
}

#[derive(Debug, Clone)]
pub struct Sticker {
    pub pack_id: Vec<u8>,
    pub sticker_id: u32,
    pub emoji: Option<String>,
    pub image: Option<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub author: Uuid,
    pub emoji: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Sending,
    Failed,
    Sent,
    Delivered,
    Read,
    Viewed,
}

impl Message {
    pub fn timestamp(&self) -> u64 {
        self.id.timestamp
    }

    pub fn sender(&self) -> Uuid {
        self.id.sender
    }

    pub fn text(&self) -> Option<&str> {
        match &self.content {
            Content::Text { body, .. } => Some(body),
            _ => None,
        }
    }

    pub fn ranges(&self) -> &[Range] {
        match &self.content {
            Content::Text { ranges, .. } => ranges,
            _ => &[],
        }
    }

    /// One-line rendering for the sidebar and notifications.
    pub fn summary(&self) -> String {
        match &self.content {
            Content::Text { body, .. } if !body.is_empty() => body.replace('\n', " "),
            Content::Text { .. } => self.attachment_summary().into(),
            Content::Sticker(sticker) => match &sticker.emoji {
                Some(emoji) => format!("{emoji} Sticker"),
                None => "Sticker".into(),
            },
            Content::Deleted => "This message was deleted".into(),
            Content::Update(Update::ExpireTimer { seconds: 0 }) => {
                "Disappearing messages off".into()
            }
            Content::Update(Update::ExpireTimer { .. }) => "Disappearing messages on".into(),
            Content::Update(Update::IdentityChanged) => "Safety number changed".into(),
            Content::Update(Update::Group(change)) => change.clone(),
        }
    }

    fn attachment_summary(&self) -> &'static str {
        use crate::data::attachment::Kind;

        match self.attachments.as_slice() {
            [] => "",
            [one] => match one.kind {
                Kind::Image { .. } => "Photo",
                Kind::Video { .. } => "Video",
                Kind::Audio {
                    voice_note: true, ..
                } => "Voice message",
                Kind::Audio { .. } => "Audio",
                Kind::File => "File",
            },
            _ => "Attachments",
        }
    }

    pub fn plain(id: MessageId, body: String) -> Self {
        Self::new(
            id,
            Content::Text {
                body,
                ranges: Vec::new(),
            },
        )
    }

    fn new(id: MessageId, content: Content) -> Self {
        Self {
            id,
            content,
            attachments: Vec::new(),
            quote: None,
            preview: None,
            reactions: Vec::new(),
            status: None,
            edited: None,
            expires_in: None,
            view_once: false,
        }
    }
}
