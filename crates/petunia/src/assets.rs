//! Where an icon comes from.
//!
//! The widget library's set is the bulk of it. Petunia adds the handful of
//! glyphs that set does not ship, drawn in the same style so nothing in the
//! chrome looks borrowed from somewhere else.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

/// Petunia's own, by the path an `Icon` asks for. Most of them are the emoji
/// picker's categories, which are drawn as icons rather than as one emoji each:
/// a rail of nine emoji is a rail of nine things the grid below it is also full
/// of, and the eye cannot tell the label from the contents.
const OWN: [(&str, &str); 10] = [
    (
        "icons/sticker.svg",
        include_str!("../assets/icons/sticker.svg"),
    ),
    ("icons/emoji.svg", include_str!("../assets/icons/emoji.svg")),
    ("icons/person.svg", include_str!("../assets/icons/person.svg")),
    ("icons/paw.svg", include_str!("../assets/icons/paw.svg")),
    ("icons/mug.svg", include_str!("../assets/icons/mug.svg")),
    ("icons/plane.svg", include_str!("../assets/icons/plane.svg")),
    ("icons/ball.svg", include_str!("../assets/icons/ball.svg")),
    ("icons/bulb.svg", include_str!("../assets/icons/bulb.svg")),
    ("icons/hash.svg", include_str!("../assets/icons/hash.svg")),
    ("icons/flag.svg", include_str!("../assets/icons/flag.svg")),
];

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
