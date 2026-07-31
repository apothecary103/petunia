pub mod markup;
pub mod project;
pub mod range;

use uuid::Uuid;

pub use project::{Fragment, classify, pointers, project, receipt_from_content};
pub use range::Range;

use super::attachment::{self, Attachment};

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
    pub quote: Option<Box<Quote>>,
    pub preview: Option<Box<LinkPreview>>,
    pub reactions: Vec<Reaction>,
    pub status: Option<Status>,
    pub edited: Option<u64>,
    pub expires_in: Option<u32>,
    pub view_once: bool,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text { body: String, ranges: Vec<Range> },
    Sticker(Box<Sticker>),
    Deleted,
    Update(Update),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    ExpireTimer { seconds: u32 },
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
    /// What installing the pack needs, and the only place it is ever sent.
    pub pack_key: Option<Vec<u8>>,
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

    /// Attachments live in four places on a message, and the same digest can
    /// appear in more than one, so every match is updated.
    pub fn set_blob(&mut self, id: &attachment::Id, blob: attachment::Blob) -> bool {
        let mut found = false;
        for attached in self.attachments_mut() {
            if attached.id == *id {
                attached.blob = blob.clone();
                found = true;
            }
        }
        found
    }

    /// Records what an image turned out to be, for the senders who do not say.
    /// Without it the layout has only the maximum box to go on and stretches a
    /// photo to fill it.
    pub fn set_image_size(&mut self, id: &attachment::Id, measured: attachment::Size) {
        for attached in self.attachments_mut() {
            if attached.id == *id
                && let attachment::Kind::Image { size, .. } = &mut attached.kind
                && size.is_none()
            {
                *size = Some(measured);
            }
        }
    }

    /// Records the still generated for a video once it is on disk.
    pub fn set_poster(&mut self, id: &attachment::Id, path: std::path::PathBuf) {
        for attached in self.attachments_mut() {
            if attached.id == *id
                && let attachment::Kind::Video { poster, .. } = &mut attached.kind
            {
                *poster = Some(path.clone());
            }
        }
    }

    /// Every attachment on the message, including the ones hanging off a quote,
    /// a link preview or a sticker.
    pub fn attachment_refs(&self) -> impl Iterator<Item = &Attachment> {
        let sticker = match &self.content {
            Content::Sticker(sticker) => sticker.image.as_ref(),
            _ => None,
        };

        self.attachments
            .iter()
            .chain(self.quote.as_ref().and_then(|quote| quote.thumbnail.as_ref()))
            .chain(self.preview.as_ref().and_then(|preview| preview.image.as_ref()))
            .chain(sticker)
    }

    fn attachments_mut(&mut self) -> impl Iterator<Item = &mut Attachment> {
        let sticker = match &mut self.content {
            Content::Sticker(sticker) => sticker.image.as_mut(),
            _ => None,
        };

        self.attachments
            .iter_mut()
            .chain(self.quote.as_mut().and_then(|quote| quote.thumbnail.as_mut()))
            .chain(self.preview.as_mut().and_then(|preview| preview.image.as_mut()))
            .chain(sticker)
    }

    /// Whether a reply, reaction or edit can target this. A tombstone and a
    /// system line are neither.
    pub fn is_addressable(&self) -> bool {
        !matches!(self.content, Content::Deleted | Content::Update(_))
    }

    pub fn mentions(&self, uuid: Uuid) -> bool {
        self.ranges()
            .iter()
            .any(|range| range.style == range::Style::Mention(uuid))
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
        }
    }

    fn attachment_summary(&self) -> &'static str {
        use crate::attachment::Kind;

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
        Self::written(id, body, Vec::new())
    }

    pub fn written(id: MessageId, body: String, ranges: Vec<Range>) -> Self {
        Self::new(id, Content::Text { body, ranges })
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

