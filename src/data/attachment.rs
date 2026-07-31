use std::path::PathBuf;
use std::time::Duration;

use presage::libsignal_service::content::AttachmentPointerFlags;
use presage::libsignal_service::proto::AttachmentPointer;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(String);

impl Id {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: Id,
    pub kind: Kind,
    pub content_type: String,
    pub file_name: Option<String>,
    pub size: u64,
    pub caption: Option<String>,
    pub blob: Blob,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Image {
        size: Option<Size>,
        blurhash: Option<String>,
    },
    Video {
        size: Option<Size>,
        duration: Option<Duration>,
    },
    Audio {
        duration: Option<Duration>,
        waveform: Option<Vec<u8>>,
        voice_note: bool,
    },
    File,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Blob {
    Missing,
    Downloading(f32),
    Cached(PathBuf),
    Failed(String),
}

/// `get_attachment` verifies the digest before decrypting and errors without
/// one, so a pointer that lacks it can never be downloaded.
pub fn from_pointer(pointer: &AttachmentPointer) -> Option<Attachment> {
    let id = Id(hex(pointer.digest.as_deref()?));
    let content_type = pointer.content_type().to_string();

    Some(Attachment {
        id,
        kind: kind(pointer, &content_type),
        content_type,
        file_name: pointer.file_name.clone(),
        size: pointer.size() as u64,
        caption: pointer.caption.clone(),
        blob: Blob::Missing,
    })
}

fn kind(pointer: &AttachmentPointer, content_type: &str) -> Kind {
    let size = size(pointer);

    if is_voice_note(pointer) {
        return Kind::Audio {
            duration: None,
            waveform: None,
            voice_note: true,
        };
    }

    match content_type.split('/').next().unwrap_or_default() {
        "image" => Kind::Image {
            size,
            blurhash: pointer.blur_hash.clone(),
        },
        "video" => Kind::Video {
            size,
            duration: None,
        },
        "audio" => Kind::Audio {
            duration: None,
            waveform: None,
            voice_note: false,
        },
        _ => Kind::File,
    }
}

fn size(pointer: &AttachmentPointer) -> Option<Size> {
    match (pointer.width(), pointer.height()) {
        (0, _) | (_, 0) => None,
        (width, height) => Some(Size { width, height }),
    }
}

fn is_voice_note(pointer: &AttachmentPointer) -> bool {
    pointer.flags() & AttachmentPointerFlags::VoiceMessage as u32 != 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(content_type: &str) -> AttachmentPointer {
        AttachmentPointer {
            digest: Some(vec![0xab, 0xcd]),
            content_type: Some(content_type.into()),
            size: Some(42),
            ..Default::default()
        }
    }

    #[test]
    fn derives_a_content_addressed_id_from_the_digest() {
        let attachment = from_pointer(&pointer("image/png")).unwrap();

        assert_eq!(attachment.id.as_str(), "abcd");
        assert_eq!(attachment.size, 42);
        assert_eq!(attachment.blob, Blob::Missing);
    }

    #[test]
    fn rejects_a_pointer_without_a_digest() {
        let pointer = AttachmentPointer {
            content_type: Some("image/png".into()),
            ..Default::default()
        };

        assert!(from_pointer(&pointer).is_none());
    }

    #[test]
    fn classifies_by_content_type() {
        let kinds = ["image/png", "video/mp4", "audio/aac", "application/pdf"]
            .map(|mime| from_pointer(&pointer(mime)).unwrap().kind);

        assert!(matches!(kinds[0], Kind::Image { .. }));
        assert!(matches!(kinds[1], Kind::Video { .. }));
        assert!(matches!(
            kinds[2],
            Kind::Audio {
                voice_note: false,
                ..
            }
        ));
        assert!(matches!(kinds[3], Kind::File));
    }

    #[test]
    fn classifies_a_voice_note_by_flag_not_mime() {
        let mut pointer = pointer("audio/aac");
        pointer.flags = Some(AttachmentPointerFlags::VoiceMessage as u32);

        assert!(matches!(
            from_pointer(&pointer).unwrap().kind,
            Kind::Audio {
                voice_note: true,
                ..
            }
        ));
    }

    #[test]
    fn reads_image_dimensions_when_present() {
        let mut pointer = pointer("image/png");
        pointer.width = Some(800);
        pointer.height = Some(600);

        let Kind::Image { size, .. } = from_pointer(&pointer).unwrap().kind else {
            panic!("expected an image");
        };
        assert_eq!(
            size,
            Some(Size {
                width: 800,
                height: 600
            })
        );
    }

    #[test]
    fn treats_zero_dimensions_as_unknown() {
        let Kind::Image { size, .. } = from_pointer(&pointer("image/png")).unwrap().kind else {
            panic!("expected an image");
        };
        assert_eq!(size, None);
    }
}
