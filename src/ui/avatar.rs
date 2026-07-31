use std::path::Path;

use gpui::prelude::*;
use gpui::{Hsla, IntoElement, SharedString, img, px};

use crate::config::Theme;

/// A cached avatar, or the first letter of the name on a colour picked from the
/// seed so the same person is always the same colour.
pub fn avatar(
    path: Option<&Path>,
    name: &str,
    seed: &[u8],
    size: f32,
    theme: &Theme,
) -> gpui::AnyElement {
    let tint = theme.accent_for(seed);

    match path {
        Some(path) => img(path.to_path_buf())
            .size(px(size))
            .rounded_full()
            .into_any_element(),
        None => gpui::div()
            .size(px(size))
            .rounded_full()
            .bg(faded(tint))
            .text_color(tint)
            .text_size(px(size * 0.42))
            .flex()
            .items_center()
            .justify_center()
            .child(SharedString::from(initial(name)))
            .into_any_element(),
    }
}

fn initial(name: &str) -> String {
    name.chars()
        .find(|character| character.is_alphanumeric())
        .map(|character| character.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

/// A tinted disc rather than a solid one: a full-strength accent behind a
/// letter reads as an alert, not as an identity.
fn faded(color: Hsla) -> Hsla {
    Hsla { a: 0.18, ..color }
}
