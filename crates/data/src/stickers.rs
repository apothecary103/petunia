//! Installed sticker packs, as the picker reads them.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Pack {
    pub id: Vec<u8>,
    pub key: Vec<u8>,
    pub title: String,
    pub author: String,
    pub stickers: Vec<Sticker>,
}

#[derive(Debug, Clone)]
pub struct Sticker {
    pub id: u32,
    pub emoji: Option<String>,
    /// presage keeps the decrypted bytes in its own store; nothing can draw
    /// bytes, so they are written into the media cache and referred to by path.
    pub path: PathBuf,
}

impl Pack {
    /// What stands in for the pack in a list of packs.
    pub fn cover(&self) -> Option<&Sticker> {
        self.stickers.first()
    }

    /// Stickers whose emoji or whose pack matches what was typed. An empty query
    /// matches everything, so the picker opens as a browser before it is a
    /// search box.
    pub fn matching(&self, query: &str) -> Vec<&Sticker> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return self.stickers.iter().collect();
        }
        // The pack's own name matching means typing its name shows all of it,
        // which is how you find a sticker you remember by pack and not by face.
        if self.title.to_lowercase().contains(&query) || self.author.to_lowercase().contains(&query)
        {
            return self.stickers.iter().collect();
        }
        self.stickers
            .iter()
            .filter(|sticker| {
                sticker
                    .emoji
                    .as_deref()
                    .is_some_and(|emoji| emoji.contains(&query) || query.contains(emoji))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> Pack {
        Pack {
            id: vec![1, 2],
            key: vec![3, 4],
            title: "Bandit the Cat".into(),
            author: "Signal".into(),
            stickers: vec![
                Sticker {
                    id: 0,
                    emoji: Some("😀".into()),
                    path: PathBuf::from("/tmp/0.webp"),
                },
                Sticker {
                    id: 1,
                    emoji: Some("😢".into()),
                    path: PathBuf::from("/tmp/1.webp"),
                },
                Sticker {
                    id: 2,
                    emoji: None,
                    path: PathBuf::from("/tmp/2.webp"),
                },
            ],
        }
    }

    #[test]
    fn an_empty_query_shows_the_whole_pack() {
        assert_eq!(pack().matching("").len(), 3);
        assert_eq!(pack().matching("   ").len(), 3);
    }

    #[test]
    fn an_emoji_finds_the_sticker_that_carries_it() {
        let pack = pack();
        let found = pack.matching("😢");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, 1);
    }

    /// Remembering a sticker by its pack rather than by its face is the common
    /// case once you have more than one pack.
    #[test]
    fn the_pack_name_matches_every_sticker_in_it() {
        assert_eq!(pack().matching("bandit").len(), 3);
        assert_eq!(pack().matching("signal").len(), 3);
    }

    #[test]
    fn nothing_matching_finds_nothing() {
        assert!(pack().matching("aardvark").is_empty());
    }

    #[test]
    fn the_cover_is_the_first_sticker() {
        assert_eq!(pack().cover().map(|sticker| sticker.id), Some(0));
    }
}
