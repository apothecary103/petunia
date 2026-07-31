use gpui::{Hsla, Rgba};
use serde::{Deserialize, Deserializer};

/// Petunia's palette and typography, read from
/// `~/.config/petunia/themes/<name>.toml` and installed as a gpui global.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    /// Self-description only; the file name is what `theme = "..."` selects.
    pub name: String,
    /// Left unset, the background's luma decides.
    pub appearance: Option<Appearance>,

    /// Window.
    #[serde(deserialize_with = "hex")]
    pub background: Hsla,
    /// Sidebar and panels.
    #[serde(deserialize_with = "hex")]
    pub surface: Hsla,
    /// Popovers, composer, modals.
    #[serde(deserialize_with = "hex")]
    pub elevated: Hsla,
    /// Inputs and code blocks.
    #[serde(deserialize_with = "hex")]
    pub sunken: Hsla,
    #[serde(deserialize_with = "hex")]
    pub border: Hsla,
    #[serde(deserialize_with = "hex")]
    pub border_focus: Hsla,

    #[serde(deserialize_with = "hex")]
    pub text: Hsla,
    /// Timestamps and secondary labels.
    #[serde(alias = "dim", deserialize_with = "hex")]
    pub text_dim: Hsla,
    /// Placeholders and disabled controls.
    #[serde(alias = "muted", deserialize_with = "hex")]
    pub text_muted: Hsla,

    #[serde(deserialize_with = "hex")]
    pub hover: Hsla,
    #[serde(deserialize_with = "hex")]
    pub active: Hsla,
    #[serde(deserialize_with = "hex")]
    pub selected: Hsla,

    #[serde(deserialize_with = "hex")]
    pub accent: Hsla,
    #[serde(deserialize_with = "hex")]
    pub on_accent: Hsla,
    #[serde(deserialize_with = "hex")]
    pub success: Hsla,
    #[serde(deserialize_with = "hex")]
    pub warning: Hsla,
    #[serde(deserialize_with = "hex")]
    pub danger: Hsla,

    /// A `Vec` so a theme picks its own count; `accent_for` is modular.
    #[serde(deserialize_with = "hex_list")]
    pub accents: Vec<Hsla>,

    pub typography: Typography,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Dark,
    Light,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Typography {
    pub family: String,
    pub mono: String,
    pub ui_size: f32,
    pub message_size: f32,
    pub line_height: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            family: system_ui().into(),
            mono: system_mono().into(),
            ui_size: 13.0,
            message_size: 14.0,
            line_height: 1.5,
        }
    }
}

/// The platform's own interface font. Naming a specific family by default would
/// be worse everywhere it is not installed, and on macOS nothing beats the
/// system one for this.
const fn system_ui() -> &'static str {
    if cfg!(target_os = "macos") {
        ".SystemUIFont"
    } else if cfg!(target_os = "windows") {
        "Segoe UI"
    } else {
        "sans-serif"
    }
}

const fn system_mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "SF Mono"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "monospace"
    }
}

impl Default for Theme {
    fn default() -> Self {
        dark()
    }
}

impl Theme {
    /// A stable per-sender colour, so the same person is the same colour in
    /// every thread and across restarts.
    pub fn accent_for(&self, seed: &[u8]) -> Hsla {
        if self.accents.is_empty() {
            return self.accent;
        }
        let sum: usize = seed.iter().map(|byte| *byte as usize).sum();
        self.accents[sum % self.accents.len()]
    }

    /// Whether this is a light theme, which decides how the widget library
    /// derives its own shades.
    pub fn is_light(&self) -> bool {
        match self.appearance {
            Some(Appearance::Light) => true,
            Some(Appearance::Dark) => false,
            None => {
                let Rgba { r, g, b, .. } = self.background.into();
                // Rec. 601 luma: close enough to perceptual for a binary decision.
                0.299 * r + 0.587 * g + 0.114 * b > 0.5
            }
        }
    }
}

fn c(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

pub fn dark() -> Theme {
    Theme {
        name: "dark".into(),
        appearance: Some(Appearance::Dark),
        // The conversation is the deepest surface and the list sits a step above
        // it, so the eye reads the reading column as the thing behind everything
        // else rather than as another panel.
        background: c(0x0a0a0a),
        surface: c(0x151515),
        elevated: c(0x1d1d1d),
        sunken: c(0x050505),
        border: c(0x242424),
        border_focus: c(0x3d3d3d),
        text: c(0xe9e9e9),
        text_dim: c(0x8f8f8f),
        text_muted: c(0x5d5d5d),
        hover: c(0x1f1f1f),
        active: c(0x2a2a2a),
        selected: c(0x232323),
        // Near-white: the reference spends its only bright value on the send
        // button, and nothing else competes with it.
        accent: c(0xededed),
        on_accent: c(0x0a0a0a),
        success: c(0x6fcf97),
        warning: c(0xd6a545),
        danger: c(0xe5747d),
        accents: ACCENTS_DARK.iter().copied().map(c).collect(),
        typography: Typography::default(),
    }
}

pub fn light() -> Theme {
    Theme {
        name: "light".into(),
        appearance: Some(Appearance::Light),
        background: c(0xf5f5f5),
        surface: c(0xe9e9e9),
        elevated: c(0xffffff),
        sunken: c(0xdedede),
        border: c(0xd6d6d6),
        border_focus: c(0xa8a8a8),
        text: c(0x1b1b1b),
        text_dim: c(0x5c5c5c),
        text_muted: c(0x8a8a8a),
        hover: c(0xe0e0e0),
        active: c(0xd2d2d2),
        selected: c(0xdcdcdc),
        accent: c(0x1b1b1b),
        on_accent: c(0xf5f5f5),
        success: c(0x1f7a45),
        warning: c(0x8a6100),
        danger: c(0xbb3a44),
        accents: ACCENTS_LIGHT.iter().copied().map(c).collect(),
        typography: Typography::default(),
    }
}

/// Sender colours. Spread around the wheel so adjacent names stay tellable
/// apart, and held to a similar lightness so none of them shouts.
/// Sender colours are the one chromatic thing in a neutral theme, so they are
/// desaturated to sit inside it rather than on top of it.
const ACCENTS_DARK: [u32; 8] = [
    0x9ab8d8, 0x9dc9a6, 0xd6bb87, 0xd8a48c, 0xb6a6d4, 0x8cc6c2, 0xd3a3bd, 0xd49399,
];

const ACCENTS_LIGHT: [u32; 8] = [
    0x35618c, 0x2f6b41, 0x8a6100, 0x99522a, 0x5b4a8a, 0x2a6d69, 0x8a3f68, 0x9b414a,
];

/// Two themes compile in so `theme = "light"` works with no files on disk;
/// anything else is read from `~/.config/petunia/themes/<name>.toml`.
pub fn load(name: &str) -> (Theme, Option<String>) {
    match name {
        "" | "dark" => return (dark(), None),
        "light" => return (light(), None),
        _ => {}
    }

    let path = super::themes_dir().join(format!("{name}.toml"));
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<Theme>(&contents) {
            Ok(mut theme) => {
                if theme.accents.is_empty() {
                    theme.accents = dark().accents;
                }
                if theme.name.is_empty() {
                    theme.name = name.to_owned();
                }
                (theme, None)
            }
            Err(error) => (
                dark(),
                Some(format!("theme {name}: {}", error.message().replace('\n', " "))),
            ),
        },
        Err(error) => (
            dark(),
            Some(format!("theme {name} ({}): {error}", path.display())),
        ),
    }
}

fn hex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Hsla, D::Error> {
    let raw = String::deserialize(deserializer)?;
    parse_hex(&raw).ok_or_else(|| serde::de::Error::custom(format!("not a #rrggbb colour: {raw}")))
}

fn hex_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Hsla>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|raw| {
            parse_hex(&raw)
                .ok_or_else(|| serde::de::Error::custom(format!("not a #rrggbb colour: {raw}")))
        })
        .collect()
}

/// Accepts `#rgb`, `#rrggbb` and `#rrggbbaa`, with or without the hash.
pub fn parse_hex(raw: &str) -> Option<Hsla> {
    let digits = raw.trim().trim_start_matches('#');
    let byte = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();

    let packed = match digits.len() {
        3 => {
            let nibble = |at: usize| {
                let value = u32::from_str_radix(digits.get(at..at + 1)?, 16).ok()?;
                Some(value * 17)
            };
            return Some(c(nibble(0)? << 16 | nibble(1)? << 8 | nibble(2)?));
        }
        6 => {
            (byte(0)? as u32) << 16 | (byte(2)? as u32) << 8 | byte(4)? as u32
        }
        8 => {
            return Some(
                gpui::rgba(
                    (byte(0)? as u32) << 24
                        | (byte(2)? as u32) << 16
                        | (byte(4)? as u32) << 8
                        | byte(6)? as u32,
                )
                .into(),
            );
        }
        _ => return None,
    };
    Some(c(packed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_three_hex_lengths() {
        assert_eq!(parse_hex("#fff"), Some(c(0xffffff)));
        assert_eq!(parse_hex("#ff8800"), Some(c(0xff8800)));
        assert_eq!(parse_hex("112233"), Some(c(0x112233)));
    }

    #[test]
    fn an_alpha_channel_is_honoured() {
        let color = parse_hex("#00000080").unwrap();

        assert!((color.a - 128.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn rejects_nonsense() {
        for raw in ["", "#12", "#12345", "zzzzzz", "#gggggg"] {
            assert_eq!(parse_hex(raw), None, "{raw} should not parse");
        }
    }

    #[test]
    fn the_built_in_themes_disagree_about_being_light() {
        assert!(!dark().is_light());
        assert!(light().is_light());
    }

    /// With no `appearance` key the background's luma has to decide, which is
    /// what every hand-written theme file relies on.
    #[test]
    fn appearance_falls_back_to_the_background_luma() {
        let dark = Theme {
            appearance: None,
            ..dark()
        };
        let light = Theme {
            appearance: None,
            ..light()
        };

        assert!(!dark.is_light());
        assert!(light.is_light());
    }

    #[test]
    fn both_built_in_themes_have_accents() {
        assert_eq!(dark().accents.len(), 8);
        assert_eq!(light().accents.len(), 8);
    }

    #[test]
    fn the_named_built_ins_load_without_a_file() {
        for name in ["", "dark", "light"] {
            let (_, error) = load(name);
            assert!(error.is_none(), "{name} reported {error:?}");
        }
    }

    #[test]
    fn an_unknown_theme_falls_back_and_says_so() {
        let (theme, error) = load("no-such-theme-exists-here");

        assert!(error.is_some());
        assert!(!theme.is_light());
    }

    #[test]
    fn a_sender_colour_is_stable_and_in_range() {
        let theme = dark();
        let seed = [1u8, 2, 3, 4];

        assert_eq!(theme.accent_for(&seed), theme.accent_for(&seed));
        assert!(theme.accents.contains(&theme.accent_for(&seed)));
    }

    /// A theme file that lists no accents must still colour senders, so the
    /// modular index cannot divide by zero.
    #[test]
    fn no_accents_falls_back_to_the_single_accent() {
        let theme = Theme {
            accents: Vec::new(),
            ..dark()
        };

        assert_eq!(theme.accent_for(&[7]), theme.accent);
    }

    #[test]
    fn a_theme_file_round_trips_through_toml() {
        let toml = r##"
            name = "custom"
            appearance = "dark"
            background = "#101010"
            surface = "#202020"
            elevated = "#252525"
            sunken = "#000000"
            border = "#303030"
            border_focus = "#404040"
            text = "#eeeeee"
            text_dim = "#aaaaaa"
            text_muted = "#777777"
            hover = "#252525"
            active = "#303030"
            selected = "#2a2a3a"
            accent = "#00aaff"
            on_accent = "#000000"
            success = "#00ff00"
            warning = "#ffff00"
            danger = "#ff0000"
            accents = ["#00aaff", "#ff00aa"]

            [typography]
            family = "Berkeley Mono"
            ui_size = 12.0
        "##;

        let theme: Theme = toml::from_str(toml).unwrap();

        assert_eq!(theme.accents.len(), 2);
        assert_eq!(theme.accent, c(0x00aaff));
        assert_eq!(theme.typography.family, "Berkeley Mono");
        assert_eq!(theme.typography.ui_size, 12.0);
        // Untouched typography keys keep their defaults.
        assert_eq!(theme.typography.mono, Typography::default().mono);
    }

    #[test]
    fn a_theme_file_may_override_only_what_it_cares_about() {
        let theme: Theme = toml::from_str(r##"accent = "#ff0000""##).unwrap();

        assert_eq!(theme.accent, c(0xff0000));
        assert_eq!(theme.background, dark().background);
    }

    /// The palette gained `text_dim`/`text_muted` in the gpui rewrite; theme
    /// files written against the old names must keep working.
    #[test]
    fn the_old_dim_and_muted_names_still_parse() {
        let theme: Theme = toml::from_str(
            r##"
                dim = "#aaaaaa"
                muted = "#777777"
            "##,
        )
        .unwrap();

        assert_eq!(theme.text_dim, c(0xaaaaaa));
        assert_eq!(theme.text_muted, c(0x777777));
    }

    #[test]
    fn a_bad_colour_in_a_theme_file_is_an_error() {
        assert!(toml::from_str::<Theme>(r#"accent = "not-a-colour""#).is_err());
    }

    #[test]
    fn an_unknown_appearance_is_an_error() {
        assert!(toml::from_str::<Theme>(r#"appearance = "twilight""#).is_err());
    }

    /// The shipped example is the format's documentation, so it must parse and
    /// must not drift from the struct definition.
    #[test]
    fn the_example_theme_parses() {
        let example = include_str!("../../themes.example.toml");

        let theme: Theme = toml::from_str(example).expect("example theme parses");

        assert_eq!(theme.name, "my-theme");
        assert_eq!(theme.appearance, Some(Appearance::Dark));
        assert!(!theme.is_light());
        assert_eq!(theme.accents.len(), 8);
        assert_eq!(theme.typography.ui_size, 13.0);
    }
}
