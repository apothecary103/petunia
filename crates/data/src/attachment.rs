use std::path::{Path, PathBuf};
use std::time::Duration;

use presage::libsignal_service::content::AttachmentPointerFlags;
use presage::libsignal_service::proto::AttachmentPointer;

use crate::hex;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(String);

impl Id {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_hex(hex: impl Into<String>) -> Self {
        Self(hex.into())
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

impl Kind {
    /// What to call it in a line of text -- a sidebar preview, or a quote of a
    /// message that was a picture rather than words. One vocabulary, so the two
    /// never call the same thing by different names.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Image { .. } => "Photo",
            Self::Video { .. } => "Video",
            Self::Audio {
                voice_note: true, ..
            } => "Voice message",
            Self::Audio { .. } => "Audio",
            Self::File => "File",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Kind {
    Image {
        size: Option<Size>,
    },
    Video {
        size: Option<Size>,
        duration: Option<Duration>,
        /// A still from the video, generated locally once it is downloaded.
        /// Signal sends no thumbnail for video, so without one a clip is a grey
        /// rectangle until you play it.
        poster: Option<PathBuf>,
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
    /// In flight, and how far in — a fraction of the declared size, or `None`
    /// for the first moment before anything has arrived and for a pointer that
    /// declares no size at all.
    ///
    /// The bytes were always there; the counting was not. presage read the
    /// whole stream in one call and reported when it was finished, which is a
    /// bar that can only slide, so `get_attachment_reporting`
    /// (`vendor/presage`) reads it in chunks and says how far it has got.
    Downloading(Option<f32>),
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

/// An attachment we are about to send. The bytes are already on disk, so the
/// echo renders before the upload finishes; the id is the path because the digest
/// only exists once the upload returns a pointer.
pub fn from_path(path: PathBuf, size: u64) -> Attachment {
    let content_type = content_type(&path);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());

    Attachment {
        id: Id(path.to_string_lossy().into_owned()),
        kind: classify(&content_type, None),
        content_type,
        file_name,
        size,
        caption: None,
        blob: Blob::Cached(path),
    }
}

/// Recipients render an attachment from the type we declare rather than by
/// sniffing the bytes, so a photo sent as `application/octet-stream` arrives as a
/// file to download instead of a picture.
pub fn content_type(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "m4a" | "aac" | "alac" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        // The lossless formats, which are the ones with a bit depth worth
        // reporting -- and the ones that went out as octet-stream and arrived as
        // a file to download rather than as something to play.
        "flac" => "audio/flac",
        "aiff" | "aif" => "audio/aiff",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// An image's real dimensions, read from its header rather than by decoding it,
/// so this costs a few hundred bytes off disk however large the picture is.
pub fn dimensions(path: &Path) -> Option<Size> {
    let (width, height) = image::image_dimensions(path).ok()?;
    Some(Size { width, height })
}

fn kind(pointer: &AttachmentPointer, content_type: &str) -> Kind {
    if is_voice_note(pointer) {
        return Kind::Audio {
            duration: None,
            waveform: None,
            voice_note: true,
        };
    }

    classify(content_type, size(pointer))
}

/// What a content type makes something, which is all a quote gets told about the
/// message it answers: Signal sends the type and the file name beside the
/// thumbnail, and no dimensions.
pub fn classify(content_type: &str, size: Option<Size>) -> Kind {
    match content_type.split('/').next().unwrap_or_default() {
        "image" => Kind::Image { size },
        "video" => Kind::Video {
            size,
            duration: None,
            poster: None,
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
    fn declares_a_content_type_from_the_extension() {
        assert_eq!(content_type(Path::new("/tmp/cat.JPEG")), "image/jpeg");
        assert_eq!(content_type(Path::new("/tmp/note.pdf")), "application/pdf");
        assert_eq!(
            content_type(Path::new("/tmp/mystery")),
            "application/octet-stream"
        );
    }

    #[test]
    fn a_local_attachment_is_cached_before_it_is_uploaded() {
        let attached = from_path(PathBuf::from("/tmp/cat.png"), 900);

        assert!(matches!(attached.kind, Kind::Image { size: None, .. }));
        assert_eq!(attached.file_name.as_deref(), Some("cat.png"));
        assert_eq!(attached.content_type, "image/png");
        assert_eq!(attached.blob, Blob::Cached(PathBuf::from("/tmp/cat.png")));
    }

    /// The digest only exists after the upload, so the pre-upload id must not
    /// look like one -- a collision would let a download overwrite the echo.
    #[test]
    fn a_local_attachment_is_not_keyed_by_a_digest() {
        let local = from_path(PathBuf::from("/tmp/cat.png"), 900);

        assert_ne!(local.id, from_pointer(&pointer("image/png")).unwrap().id);
    }

    #[test]
    fn treats_zero_dimensions_as_unknown() {
        let Kind::Image { size, .. } = from_pointer(&pointer("image/png")).unwrap().kind else {
            panic!("expected an image");
        };
        assert_eq!(size, None);
    }
}
