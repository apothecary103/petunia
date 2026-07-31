//! Where an icon comes from.
//!
//! The widget library's set is the bulk of it. Petunia adds the handful of
//! glyphs that set does not ship, drawn in the same style so nothing in the
//! chrome looks borrowed from somewhere else.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

/// Petunia's own, by the path an `Icon` asks for.
const OWN: [(&str, &str); 1] = [(
    "icons/sticker.svg",
    include_str!("../assets/icons/sticker.svg"),
)];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match OWN.iter().find(|(name, _)| *name == path) {
            Some((_, svg)) => Ok(Some(Cow::Borrowed(svg.as_bytes()))),
            None => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut listed = gpui_component_assets::Assets.list(path)?;
        listed.extend(
            OWN.iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        Ok(listed)
    }
}
