//! The stickers somebody keeps to hand.
//!
//! Its own file rather than a field in `session.json`: the session is written
//! once, on quit, by the workspace, and a favourite is kept the moment it is
//! picked from a menu three views down. Two writers of one file is one of them
//! losing.

use std::fs;
use std::path::PathBuf;

use tracing::warn;

use petunia_data::stickers::Favourite;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Favourites(Vec<Favourite>);

impl Favourites {
    pub fn load() -> Self {
        fs::read_to_string(path())
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default()
    }

    pub fn kept(&self) -> &[Favourite] {
        &self.0
    }

    pub fn holds(&self, pack_id: &[u8], sticker: u32) -> bool {
        self.0.iter().any(|kept| kept.is(pack_id, sticker))
    }

    /// Keeps a sticker, or lets it go if it was already kept. One verb, because
    /// the menu that calls this draws one entry either way.
    pub fn toggle(&mut self, pack_id: &[u8], sticker: u32) {
        self.flip(pack_id, sticker);
        self.save();
    }

    fn flip(&mut self, pack_id: &[u8], sticker: u32) {
        match self.holds(pack_id, sticker) {
            true => self.0.retain(|kept| !kept.is(pack_id, sticker)),
            false => self.0.push(Favourite::new(pack_id, sticker)),
        }
    }

    fn save(&self) {
        let path = path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let contents = serde_json::to_string_pretty(self).expect("favourites are serializable");
        if let Err(error) = fs::write(&path, contents) {
            warn!(%error, path = %path.display(), "failed to save the favourite stickers");
        }
    }
}

fn path() -> PathBuf {
    petunia_config::dir().join("favourites.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same gesture keeps one and lets it go again, because the menu draws
    /// one entry either way.
    #[test]
    fn keeping_one_twice_lets_it_go() {
        let mut favourites = Favourites::default();

        favourites.flip(&[1, 2], 3);
        assert!(favourites.holds(&[1, 2], 3));

        favourites.flip(&[1, 2], 3);
        assert!(!favourites.holds(&[1, 2], 3));
    }

    /// Two packs whose ids agree at the front are two packs, and a sticker in
    /// one is not a sticker in the other.
    #[test]
    fn a_favourite_belongs_to_one_pack() {
        let mut favourites = Favourites::default();

        favourites.flip(&[1, 2], 3);

        assert!(!favourites.holds(&[1, 2, 3], 3));
        assert!(!favourites.holds(&[1, 2], 4));
    }

    /// The order they were kept in is the order they are drawn in.
    #[test]
    fn the_order_they_were_kept_in_is_remembered() {
        let mut favourites = Favourites::default();

        favourites.flip(&[1], 1);
        favourites.flip(&[1], 2);

        assert_eq!(
            favourites.kept().iter().map(|kept| kept.sticker).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }
}
