use std::sync::{Arc, RwLock};

use iced::theme::{Palette, palette};
use iced::widget::{button, container, rule, text, text_editor, text_input};
use iced::{Background, Border, Color, Font, Theme, border, font};

pub use crate::config::theme::Colors;

/// The active palette. A global because every style function in iced takes only
/// `&Theme`, which carries six colours and no room for petunia's twenty.
static COLORS: RwLock<Option<Arc<Colors>>> = RwLock::new(None);

pub fn install(colors: Arc<Colors>) {
    if let Ok(mut slot) = COLORS.write() {
        *slot = Some(colors);
    }
}

/// `Arc` derefs, so `theme::colors().accent` reads the same as before.
pub fn colors() -> Arc<Colors> {
    if let Ok(slot) = COLORS.read()
        && let Some(colors) = slot.as_ref()
    {
        return colors.clone();
    }
    Arc::new(crate::config::theme::mocha())
}

pub const FONT_BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..Font::MONOSPACE
};

pub const FONT_ITALIC: Font = Font {
    style: font::Style::Italic,
    ..Font::MONOSPACE
};

/// Named separately from the default so that an inline-code range still says what
/// it is, whichever family the shell is set to.
pub const FONT_MONO: Font = Font::MONOSPACE;

pub fn accent(seed: &[u8]) -> Color {
    colors().accent_for(seed)
}

/// Built through `custom_with_fn` so the extended palette iced derives for its
/// built-in widgets comes from the real surface and border colours rather than
/// from `Palette::generate`'s guesses.
pub fn build() -> Theme {
    let colors = colors();
    let palette = Palette {
        background: colors.background,
        text: colors.text,
        primary: colors.accent,
        success: colors.success,
        warning: colors.warning,
        danger: colors.danger,
    };

    Theme::custom_with_fn("Petunia".to_owned(), palette, move |palette| {
        let mut extended = palette::Extended::generate(palette);
        extended.is_dark = !colors.is_light();
        extended.background.weak.color = colors.surface;
        extended.background.weakest.color = colors.sunken;
        extended.background.strong.color = colors.border;
        extended
    })
}

pub fn pane(_theme: &Theme, focused: bool) -> container::Style {
    let colors = colors();
    container::Style {
        background: Some(Background::Color(colors.surface)),
        border: Border {
            width: 1.0,
            color: if focused { colors.accent } else { colors.border },
            radius: border::radius(9),
        },
        ..container::Style::default()
    }
}

pub fn separator(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: colors().border,
        radius: border::radius(0),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

pub fn message_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let colors = colors();
    text_input::Style {
        background: Background::Color(colors.sunken),
        border: Border {
            width: 1.0,
            color: match status {
                text_input::Status::Focused { .. } => colors.accent,
                _ => colors.border,
            },
            radius: border::radius(6),
        },
        icon: colors.dim,
        placeholder: colors.dim,
        value: colors.text,
        selection: Color {
            a: 0.3,
            ..colors.accent
        },
    }
}

pub fn composer(_theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let colors = colors();
    text_editor::Style {
        background: Background::Color(colors.sunken),
        border: Border {
            width: 1.0,
            color: match status {
                text_editor::Status::Focused { .. } => colors.accent,
                _ => colors.border,
            },
            radius: border::radius(6),
        },
        placeholder: colors.dim,
        value: colors.text,
        selection: Color {
            a: 0.3,
            ..colors.accent
        },
    }
}

pub fn sidebar_entry(_theme: &Theme, status: button::Status) -> button::Style {
    let colors = colors();
    button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => {
                Some(Background::Color(colors.border))
            }
            _ => None,
        },
        text_color: colors.text,
        border: border::rounded(6),
        ..button::Style::default()
    }
}

/// The selected row, which must read as selected without a hover.
pub fn sidebar_selected(_theme: &Theme, _status: button::Status) -> button::Style {
    let colors = colors();
    button::Style {
        background: Some(Background::Color(Color {
            a: 0.22,
            ..colors.accent
        })),
        text_color: colors.text,
        border: border::rounded(6),
        ..button::Style::default()
    }
}

pub fn pane_control(_theme: &Theme, status: button::Status) -> button::Style {
    let colors = colors();
    button::Style {
        text_color: match status {
            button::Status::Hovered | button::Status::Pressed => colors.text,
            _ => colors.muted,
        },
        ..button::Style::default()
    }
}

/// A raised surface for overlays: the quick switcher, menus, notices.
pub fn overlay(_theme: &Theme) -> container::Style {
    let colors = colors();
    container::Style {
        background: Some(Background::Color(colors.surface)),
        border: Border {
            width: 1.0,
            color: colors.border,
            radius: border::radius(10),
        },
        ..container::Style::default()
    }
}

pub fn chip(_theme: &Theme) -> container::Style {
    let colors = colors();
    container::Style {
        background: Some(Background::Color(colors.sunken)),
        border: Border {
            width: 1.0,
            color: colors.border,
            radius: border::radius(4),
        },
        ..container::Style::default()
    }
}

pub fn unread_badge(_theme: &Theme) -> container::Style {
    let colors = colors();
    container::Style {
        background: Some(Background::Color(colors.danger)),
        text_color: Some(colors.on_accent),
        border: border::rounded(8),
        ..container::Style::default()
    }
}

pub fn error_banner(_theme: &Theme) -> container::Style {
    let colors = colors();
    container::Style {
        background: Some(Background::Color(colors.danger)),
        text_color: Some(colors.on_accent),
        ..container::Style::default()
    }
}

pub fn text_dim(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors().dim),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The global starts empty, and a style function must not panic before the
    /// config has loaded.
    #[test]
    fn colors_are_available_before_install() {
        assert!(!colors().accents.is_empty());
    }

    #[test]
    fn installing_swaps_the_palette() {
        install(Arc::new(crate::config::theme::latte()));
        assert!(colors().is_light());

        install(Arc::new(crate::config::theme::mocha()));
        assert!(!colors().is_light());
    }

    #[test]
    fn the_derived_palette_follows_the_installed_theme() {
        install(Arc::new(crate::config::theme::latte()));
        let light = build();
        assert!(!light.extended_palette().is_dark);

        install(Arc::new(crate::config::theme::mocha()));
        assert!(build().extended_palette().is_dark);
    }
}
