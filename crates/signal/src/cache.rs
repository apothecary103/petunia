use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::io::AsyncReadExt;

use super::Error;
use petunia_data::Thread;
use petunia_data::attachment::Id;

/// Enough bytes for every image magic number `guess_format` recognises.
const MAGIC: usize = 32;

/// Content-addressed store for attachment and avatar bytes. presage persists
/// attachment pointers but never their bytes, and Signal's CDN expires entries
/// after a few weeks, so anything not kept here is gone for good.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
    /// Bytes of cached media to keep before the oldest entries are dropped.
    limit: u64,
}

impl Cache {
    pub fn new(limit_mb: u32) -> Self {
        Self {
            root: petunia_config::cache_dir(),
            limit: u64::from(limit_mb) * 1024 * 1024,
        }
    }

    fn attachments(&self, id: &Id) -> PathBuf {
        self.root
            .join("attachments")
            .join(id.as_str().get(..2).unwrap_or("00"))
    }

    fn avatars(&self) -> PathBuf {
        self.root.join("avatars")
    }

    fn stickers(&self, pack_id: &[u8]) -> PathBuf {
        self.root.join("stickers").join(hex(pack_id))
    }

    /// A sticker from an installed pack. presage keeps the decrypted bytes in
    /// its own store, but nothing can draw bytes -- the renderer wants a path.
    pub async fn put_sticker(
        &self,
        pack_id: &[u8],
        sticker_id: u32,
        bytes: &[u8],
    ) -> Result<PathBuf, Error> {
        let path = self
            .stickers(pack_id)
            .join(file(&sticker_id.to_string(), sniff(bytes)));
        write(&path, bytes).await?;
        Ok(path)
    }

    pub async fn sticker(&self, pack_id: &[u8], sticker_id: u32) -> Option<PathBuf> {
        found(&self.stickers(pack_id), &sticker_id.to_string()).await
    }

    fn posters(&self) -> PathBuf {
        self.root.join("posters")
    }

    /// A still from a video, generated here rather than sent: Signal carries no
    /// thumbnail for video, so without one a clip is a grey rectangle.
    pub async fn put_poster(&self, id: &Id, bytes: &[u8]) -> Result<PathBuf, Error> {
        let path = self.posters().join(file(id.as_str(), sniff(bytes)));
        write(&path, bytes).await?;
        Ok(path)
    }

    pub async fn poster(&self, id: &Id) -> Option<PathBuf> {
        found(&self.posters(), id.as_str()).await
    }

    /// The path bytes land on, named for what they actually are rather than for
    /// what the sender claimed they were.
    fn attachment_path(&self, id: &Id, name: Option<&str>) -> PathBuf {
        self.attachments(id).join(file(id.as_str(), name))
    }

    pub async fn attachment(&self, id: &Id) -> Option<PathBuf> {
        found(&self.attachments(id), id.as_str()).await
    }

    pub async fn avatar(&self, thread: &Thread) -> Option<PathBuf> {
        found(&self.avatars(), &avatar_key(thread)).await
    }

    pub async fn put_attachment(
        &self,
        id: &Id,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, Error> {
        let path = self.attachment_path(id, extension(bytes, content_type));
        write(&path, bytes).await?;
        Ok(path)
    }

    /// Adopts a file that is already on disk, for media we sent ourselves: the
    /// CDN expires pointers after a few weeks, so without this our own sent
    /// attachments become unrecoverable when history is reloaded later.
    pub async fn adopt_attachment(
        &self,
        id: &Id,
        content_type: &str,
        source: &Path,
    ) -> Result<PathBuf, Error> {
        let head = head(source).await.unwrap_or_default();
        let path = self.attachment_path(id, extension(&head, content_type));
        let partial = stage(&path).await?;
        fs::copy(source, &partial).await?;
        fs::rename(&partial, &path).await?;
        Ok(path)
    }

    pub async fn put_avatar(&self, thread: &Thread, bytes: &[u8]) -> Result<PathBuf, Error> {
        let path = self
            .avatars()
            .join(file(&avatar_key(thread), sniff(bytes)));
        write(&path, bytes).await?;
        Ok(path)
    }

    pub async fn prune(&self) -> Result<Pruned, Error> {
        self.prune_to(self.limit).await
    }

    /// Drops the least recently modified entries until the tree fits the limit.
    /// The tree is authoritative rather than the blob table, because files can go
    /// missing without petunia having deleted them.
    async fn prune_to(&self, limit: u64) -> Result<Pruned, Error> {
        let mut entries = Vec::new();
        let mut total = 0;
        collect(&self.root.join("attachments"), &mut entries, &mut total).await?;

        let mut pruned = Pruned::default();
        if total <= limit {
            return Ok(pruned);
        }

        entries.sort_by_key(|(modified, _, _)| *modified);
        for (_, path, size) in entries {
            if total - pruned.freed <= limit {
                break;
            }
            if fs::remove_file(&path).await.is_ok() {
                pruned.freed += size;
                if let Some(digest) = digest_of(&path) {
                    pruned.digests.push(digest);
                }
            }
        }
        Ok(pruned)
    }
}

/// What a prune removed, so the blob table can forget the same entries and stay
/// consistent with the tree.
#[derive(Debug, Default)]
pub struct Pruned {
    pub freed: u64,
    pub digests: Vec<Id>,
}

/// The file name is the digest, with an extension added only for the benefit of
/// external viewers.
fn digest_of(path: &Path) -> Option<Id> {
    Some(Id::from_hex(path.file_stem()?.to_str()?))
}

fn file(stem: &str, extension: Option<&str>) -> String {
    match extension {
        Some(extension) => format!("{stem}.{extension}"),
        None => stem.to_string(),
    }
}

/// The extension cannot be derived from the key, because it comes from the bytes,
/// so an entry is found by its stem and then checked.
async fn found(dir: &Path, stem: &str) -> Option<PathBuf> {
    let mut entries = fs::read_dir(dir).await.ok()?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let matches = path.file_stem().and_then(OsStr::to_str) == Some(stem);
        // A `.part` shares its stem with the file it is becoming.
        let partial = path.extension().and_then(OsStr::to_str) == Some("part");
        if matches && !partial {
            return repair(path).await;
        }
    }
    None
}

/// iced decodes an image with `image::open`, which reads the format from the
/// **file extension alone**, so a name that disagrees with the bytes does not
/// merely look wrong -- it cannot be decoded at all. Entries written before the
/// name came from the bytes are renamed on the way past.
async fn repair(path: PathBuf) -> Option<PathBuf> {
    let Some(wanted) = sniff(&head(&path).await?) else {
        // Not an image, so there is nothing to be right or wrong about.
        return Some(path);
    };
    if path.extension().and_then(OsStr::to_str) == Some(wanted) {
        return Some(path);
    }

    let fixed = path.with_extension(wanted);
    match fs::rename(&path, &fixed).await {
        Ok(()) => Some(fixed),
        Err(_) => Some(path),
    }
}

async fn head(path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).await.ok()?;
    let mut bytes = vec![0; MAGIC];
    let read = file.read(&mut bytes).await.ok()?;
    bytes.truncate(read);
    Some(bytes)
}

/// Written to a temporary name and renamed, so a crash never leaves a truncated
/// file behind that a later run would read as a complete one.
async fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let partial = stage(path).await?;
    fs::write(&partial, bytes).await?;
    fs::rename(&partial, path).await?;
    Ok(())
}

/// Creates the parent directory and returns the temporary name the bytes land on
/// before the rename.
async fn stage(path: &Path) -> Result<PathBuf, Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(path.with_extension("part"))
}

async fn collect(
    dir: &Path,
    entries: &mut Vec<(std::time::SystemTime, PathBuf, u64)>,
    total: &mut u64,
) -> Result<(), Error> {
    let Ok(mut shards) = fs::read_dir(dir).await else {
        return Ok(());
    };

    while let Some(shard) = shards.next_entry().await? {
        let Ok(mut files) = fs::read_dir(shard.path()).await else {
            continue;
        };
        while let Some(file) = files.next_entry().await? {
            let Ok(metadata) = file.metadata().await else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
            *total += metadata.len();
            entries.push((modified, file.path(), metadata.len()));
        }
    }
    Ok(())
}

fn avatar_key(thread: &Thread) -> String {
    match thread {
        Thread::Contact(contact) => contact.uuid().to_string(),
        Thread::Group(master_key) => hex(master_key),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// What the bytes say they are, falling back to what the sender said. Signal
/// declares sticker packs as `image/webp` whatever the individual stickers
/// actually are, so the declaration alone is not usable.
fn extension(bytes: &[u8], content_type: &str) -> Option<&'static str> {
    sniff(bytes).or_else(|| declared(content_type))
}

fn sniff(bytes: &[u8]) -> Option<&'static str> {
    use image::ImageFormat;

    Some(match image::guess_format(bytes).ok()? {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Ico => "ico",
        ImageFormat::Avif => "avif",
        _ => return None,
    })
}

/// So that a file handed to an external player or viewer carries a name the
/// system recognises. Only reached for the media iced never decodes itself.
fn declared(content_type: &str) -> Option<&'static str> {
    Some(match content_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "audio/aac" | "audio/mp4" | "audio/m4a" => "m4a",
        "audio/mpeg" => "mp3",
        "audio/ogg" | "audio/opus" => "ogg",
        "audio/wav" => "wav",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use petunia_data::ContactId;
    use uuid::Uuid;

    fn cache(root: &Path) -> Cache {
        Cache {
            root: root.to_path_buf(),
            limit: 2 * 1024 * 1024 * 1024,
        }
    }

    fn id(hex: &str) -> Id {
        Id::from_hex(hex)
    }

    /// Only the magic number matters: nothing here decodes the pixels.
    fn png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(64, 0);
        bytes
    }

    fn webp() -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        bytes.resize(64, 0);
        bytes
    }

    #[tokio::test]
    async fn shards_by_the_first_two_hex_digits() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache(dir.path())
            .put_attachment(&id("abcdef"), "image/png", &png())
            .await
            .unwrap();

        assert!(path.ends_with("attachments/ab/abcdef.png"), "{path:?}");
    }

    #[tokio::test]
    async fn omits_the_extension_for_an_unknown_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache(dir.path())
            .put_attachment(&id("abcdef"), "application/x-thing", b"opaque")
            .await
            .unwrap();

        assert!(path.ends_with("attachments/ab/abcdef"), "{path:?}");
    }

    /// Signal declares every sticker `image/webp` whatever it sent, and iced
    /// decodes by extension only, so trusting the declaration makes the file
    /// undecodable.
    #[tokio::test]
    async fn names_the_file_after_the_bytes_not_the_declared_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache(dir.path())
            .put_attachment(&id("abcdef"), "image/webp", &png())
            .await
            .unwrap();

        assert_eq!(path.extension().unwrap(), "png");
    }

    #[tokio::test]
    async fn names_avatars_by_thread() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let uuid = Uuid::new_v4();

        let contact = cache
            .put_avatar(&Thread::Contact(ContactId::Aci(uuid)), &webp())
            .await
            .unwrap();
        let group = cache
            .put_avatar(&Thread::Group([0xab; 32]), &png())
            .await
            .unwrap();

        assert!(contact.ends_with(format!("avatars/{uuid}.webp")), "{contact:?}");
        assert!(group.ends_with(format!("avatars/{}.png", "ab".repeat(32))));
    }

    #[tokio::test]
    async fn round_trips_an_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let id = id("beef");

        assert!(cache.attachment(&id).await.is_none());

        let path = cache.put_attachment(&id, "image/png", &png()).await.unwrap();

        assert_eq!(tokio::fs::read(&path).await.unwrap(), png());
        assert_eq!(cache.attachment(&id).await, Some(path));
    }

    #[tokio::test]
    async fn round_trips_an_avatar() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));

        assert!(cache.avatar(&thread).await.is_none());

        let path = cache.put_avatar(&thread, &png()).await.unwrap();

        assert_eq!(cache.avatar(&thread).await, Some(path));
    }

    /// Entries written before the name came from the bytes are unreadable until
    /// they are renamed, and there are real caches full of them.
    #[tokio::test]
    async fn repairs_a_legacy_entry_on_the_way_past() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));

        let legacy = dir.path().join("avatars").join(avatar_key(&thread));
        tokio::fs::create_dir_all(legacy.parent().unwrap()).await.unwrap();
        tokio::fs::write(&legacy, png()).await.unwrap();

        let found = cache.avatar(&thread).await.unwrap();

        assert_eq!(found.extension().unwrap(), "png");
        assert!(!tokio::fs::try_exists(&legacy).await.unwrap());
    }

    #[tokio::test]
    async fn leaves_a_non_image_alone() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let id = id("beef");
        cache
            .put_attachment(&id, "application/pdf", b"%PDF-1.7 not really")
            .await
            .unwrap();

        let found = cache.attachment(&id).await.unwrap();

        assert_eq!(found.extension().unwrap(), "pdf");
    }

    #[tokio::test]
    async fn leaves_no_partial_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        cache
            .put_attachment(&id("beef"), "image/png", &png())
            .await
            .unwrap();

        let shard = dir.path().join("attachments").join("be");
        let mut names: Vec<_> = std::fs::read_dir(shard)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();

        assert_eq!(names, ["beef.png"]);
    }

    #[tokio::test]
    async fn the_same_digest_is_stored_once() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());

        cache
            .put_attachment(&id("beef"), "image/png", &png())
            .await
            .unwrap();
        cache
            .put_attachment(&id("beef"), "image/png", &png())
            .await
            .unwrap();

        let shard = dir.path().join("attachments").join("be");
        assert_eq!(std::fs::read_dir(shard).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn pruning_under_the_limit_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        cache
            .put_attachment(&id("beef"), "image/png", &png())
            .await
            .unwrap();

        assert_eq!(cache.prune().await.unwrap().freed, 0);
        assert!(cache.attachment(&id("beef")).await.is_some());
    }

    #[tokio::test]
    async fn pruning_an_empty_cache_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(cache(dir.path()).prune().await.unwrap().freed, 0);
    }

    #[tokio::test]
    async fn pruning_evicts_the_oldest_entries_first() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let mut bytes = png();
        bytes.resize(100, 0);

        for (index, name) in ["aa11", "bb22", "cc33"].iter().enumerate() {
            let path = cache
                .put_attachment(&id(name), "image/png", &bytes)
                .await
                .unwrap();
            // mtime resolution is coarse, so order is forced explicitly.
            let when = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000 + index as u64 * 100);
            filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(when)).unwrap();
        }

        // Room for one file only, so the two oldest go.
        let pruned = cache.prune_to(150).await.unwrap();

        assert_eq!(pruned.freed, 200);
        assert_eq!(pruned.digests, [id("aa11"), id("bb22")]);
        assert!(cache.attachment(&id("aa11")).await.is_none());
        assert!(cache.attachment(&id("bb22")).await.is_none());
        assert!(cache.attachment(&id("cc33")).await.is_some());
    }
}



