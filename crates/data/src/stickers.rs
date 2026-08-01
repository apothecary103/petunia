//! Installed sticker packs, as the picker reads them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::hex;

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

/// A sticker somebody has kept to hand. A reference rather than a copy: the
/// bytes belong to the pack, so a pack that is removed takes its favourites out
/// of the grid with it rather than leaving tiles that draw nothing.
///
/// The pack is named in hex because this is written to a file a person may well
/// open, and an array of thirty-two numbers is not a pack id anybody can read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favourite {
    pub pack: String,
    pub sticker: u32,
}

impl Favourite {
    pub fn new(pack_id: &[u8], sticker: u32) -> Self {
        Self {
            pack: hex(pack_id),
            sticker,
        }
    }

    pub fn is(&self, pack_id: &[u8], sticker: u32) -> bool {
        self.sticker == sticker && self.pack == hex(pack_id)
    }
}


/// Where the favourites are in the packs this account has, in the order they
/// were kept. Anything belonging to a pack that is no longer installed is left
/// out, since there is nothing to draw for it.
pub fn favourites<'a>(
    packs: &'a [Pack],
    kept: &[Favourite],
) -> Vec<(&'a Pack, &'a Sticker)> {
    kept.iter()
        .filter_map(|favourite| {
            let pack = packs
                .iter()
                .find(|pack| favourite.pack == hex(&pack.id))?;
            let sticker = pack
                .stickers
                .iter()
                .find(|sticker| sticker.id == favourite.sticker)?;
            Some((pack, sticker))
        })
        .collect()
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
    fn a_favourite_names_its_pack_whole() {
        let favourite = Favourite::new(&[0xde, 0xad, 0xbe, 0xef], 7);

        assert_eq!(favourite.pack, "deadbeef");
        assert!(favourite.is(&[0xde, 0xad, 0xbe, 0xef], 7));
        assert!(!favourite.is(&[0xde, 0xad, 0xbe, 0xef], 8));
        assert!(!favourite.is(&[0xde, 0xad], 7));
    }

    #[test]
    fn favourites_resolve_in_the_order_they_were_kept() {
        let packs = vec![pack()];
        let kept = vec![Favourite::new(&[1, 2], 2), Favourite::new(&[1, 2], 0)];

        let found = favourites(&packs, &kept);

        assert_eq!(
            found.iter().map(|(_, sticker)| sticker.id).collect::<Vec<_>>(),
            vec![2, 0]
        );
    }

    /// A pack that has been removed takes its favourites out of the grid rather
    /// than leaving tiles with nothing behind them.
    #[test]
    fn a_favourite_from_a_pack_that_is_gone_is_left_out() {
        let kept = vec![Favourite::new(&[9, 9], 0), Favourite::new(&[1, 2], 9)];

        assert!(favourites(&[pack()], &kept).is_empty());
    }

    #[test]
    fn the_cover_is_the_first_sticker() {
        assert_eq!(pack().cover().map(|sticker| sticker.id), Some(0));
    }
}
