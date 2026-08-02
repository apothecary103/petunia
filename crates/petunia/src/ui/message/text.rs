//! A text attachment, shown rather than merely named.
//!
//! Signal declares almost every source file `application/octet-stream`, so what
//! a file holds is read from its name and then from its bytes. Nothing here
//! trusts the content type.

use std::path::Path;
use std::rc::Rc;

/// How much of a file is worth reading inline. Past this it is a file to open,
/// not something to glance at.
const LINES: usize = 14;
const BYTES: u64 = 512 * 1024;

/// The same question asked of an attachment, which has two names and only one of
/// them says anything.
///
/// The bytes on disk are content-addressed: a digest, with whatever extension the
/// declared content type happened to name -- and Signal declares source code
/// `application/octet-stream`, which names nothing. So a listing was drawn as a
/// listing while it was being sent, where the path is still the file we picked,
/// and went back to being a chip the moment the thread was reloaded out of the
/// cache. The name the file travelled under is the one that knows; the path is the
/// fallback, for our own attachments that have not been through the cache yet.
pub fn language_of(declared: Option<&str>, path: &Path) -> Option<&'static str> {
    declared
        .map(Path::new)
        .and_then(language)
        .or_else(|| language(path))
}

/// What the highlighter calls the language a file is written in, or `"text"` for
/// something plain. `None` means it is not text at all, and gets the chip every
/// other attachment gets.
///
/// By extension, because that is the only thing a name says. A grammar the
/// widget library does not have parses as text, which is the honest answer
/// rather than a guess dressed up in colour.
pub fn language(path: &Path) -> Option<&'static str> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let named = match extension.as_str() {
        "rs" => "rust",
        "toml" => "toml",
        "json" | "jsonc" => "json",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" | "mdx" => "markdown",
        "py" | "pyi" => "python",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "lua" => "lua",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" | "scss" => "css",
        "swift" => "swift",
        "scala" | "sbt" => "scala",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        "svelte" => "svelte",
        "astro" => "astro",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "cmake" => "cmake",
        "diff" | "patch" => "diff",
        "txt" | "text" | "log" | "csv" | "conf" | "cfg" | "ini" | "env" => "text",
        // A name with no extension at all is the build file it usually is, or
        // nothing we can read.
        "" => match stem(path).as_str() {
            "makefile" | "gnumakefile" => "make",
            "dockerfile" | "license" | "readme" | "changelog" | "notice" => "text",
            _ => return None,
        },
        _ => return None,
    };
    Some(named)
}

/// The head of a text file, and how many lines were left off the end.
///
/// Read once and kept: a visible row is rebuilt on every frame, so reading a
/// file where it is drawn would be a syscall per frame per attachment. Keyed by
/// what a file's metadata says as well as its path, because an attachment being
/// sent is a file on disk that somebody may still be editing.
pub fn head(path: &Path) -> Option<Rc<Head>> {
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Enough for what is on screen and the overdraw around it.
    const CAPACITY: usize = 32;

    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);

    thread_local! {
        static CACHE: RefCell<HashMap<Key, Option<Rc<Head>>>> = RefCell::new(HashMap::new());
    }

    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > BYTES {
        return None;
    }
    let key = (path.to_path_buf(), metadata.len(), metadata.modified().ok());

    CACHE.with(|cache| {
        if let Some(cached) = cache.borrow().get(&key) {
            return cached.clone();
        }

        let read = read(path).map(Rc::new);
        let mut cache = cache.borrow_mut();
        if cache.len() >= CAPACITY {
            cache.clear();
        }
        cache.insert(key, read.clone());
        read
    })
}

pub struct Head {
    pub code: String,
    /// What is not shown, so the preview can say so rather than simply stopping.
    pub remaining: usize,
}

/// Reads the first lines of a file, refusing anything that is not text. A NUL
/// byte is what every tool uses to tell one from the other, and invalid UTF-8 is
/// nothing this can draw.
fn read(path: &Path) -> Option<Head> {
    let contents = std::fs::read(path).ok()?;
    if contents.contains(&0) {
        return None;
    }
    let contents = String::from_utf8(contents).ok()?;

    let total = contents.lines().count();
    let code: Vec<&str> = contents.lines().take(LINES).collect();

    // A file of nothing is not worth a box around it.
    if code.iter().all(|line| line.trim().is_empty()) {
        return None;
    }

    Some(Head {
        code: code.join("\n"),
        remaining: total.saturating_sub(code.len()),
    })
}

fn stem(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_language_from_the_extension() {
        assert_eq!(language(Path::new("/tmp/main.RS")), Some("rust"));
        assert_eq!(language(Path::new("/tmp/notes.txt")), Some("text"));
        assert_eq!(language(Path::new("/tmp/Makefile")), Some("make"));
    }

    /// Everything that is not text has to fall through to the chip, or a photo
    /// would be drawn as an empty box of code.
    #[test]
    fn refuses_what_is_not_text() {
        assert_eq!(language(Path::new("/tmp/cat.png")), None);
        assert_eq!(language(Path::new("/tmp/clip.mp4")), None);
        assert_eq!(language(Path::new("/tmp/mystery")), None);
    }

    /// The cached bytes are a digest with no extension, which is what a reloaded
    /// thread hands this: the name it was sent under is the only thing left that
    /// says what it holds.
    #[test]
    fn the_declared_name_answers_for_a_cached_file() {
        let cached = Path::new("/cache/attachments/ab/abcdef");

        assert_eq!(language_of(Some("main.rs"), cached), Some("rust"));
        assert_eq!(language_of(Some("cat.png"), cached), None);
        assert_eq!(language_of(None, cached), None);
    }

    /// Our own attachments are still where we picked them and carry no declared
    /// name until the message comes back off disk.
    #[test]
    fn the_path_answers_when_nothing_was_declared() {
        assert_eq!(language_of(None, Path::new("/tmp/notes.md")), Some("markdown"));
        assert_eq!(
            language_of(Some("blob"), Path::new("/tmp/notes.md")),
            Some("markdown")
        );
    }

    #[test]
    fn reads_the_head_and_counts_the_rest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("many.txt");
        let body: String = (0..LINES + 6).map(|at| format!("line {at}\n")).collect();
        std::fs::write(&path, body).unwrap();

        let head = read(&path).expect("read");

        assert_eq!(head.code.lines().count(), LINES);
        assert_eq!(head.remaining, 6);
        assert!(head.code.starts_with("line 0"));
    }

    /// A file that fits is shown whole, with nothing claimed to be missing.
    #[test]
    fn a_short_file_has_nothing_remaining() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("short.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();

        let head = read(&path).expect("read");

        assert_eq!(head.remaining, 0);
        assert_eq!(head.code, "fn main() {}");
    }

    /// The extension is a claim, not a fact: a `.txt` full of bytes is refused
    /// here rather than drawn as replacement characters.
    #[test]
    fn binary_bytes_are_refused_whatever_the_name_says() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lying.txt");
        std::fs::write(&path, [0x68, 0x00, 0x69]).unwrap();

        assert!(read(&path).is_none());
    }

    #[test]
    fn an_empty_file_is_not_previewed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("empty.txt");
        std::fs::write(&path, "\n\n  \n").unwrap();

        assert!(read(&path).is_none());
    }

    /// The point of the cache: a row is rebuilt every frame, and the second ask
    /// must not touch the disk again.
    #[test]
    fn the_same_file_is_read_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("once.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();

        let first = head(&path).expect("read");
        let again = head(&path).expect("read");

        assert!(Rc::ptr_eq(&first, &again));
    }
}
