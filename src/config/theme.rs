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
            family: "Inter".into(),
            mono: "JetBrains Mono".into(),
            ui_size: 13.0,
            message_size: 14.0,
            line_height: 1.55,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        mocha()
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

pub fn mocha() -> Theme {
    Theme {
        name: "mocha".into(),
        appearance: Some(Appearance::Dark),
        background: c(0x181825),
        surface: c(0x1e1e2e),
        elevated: c(0x252537),
        sunken: c(0x11111b),
        border: c(0x313244),
        border_focus: c(0x45475a),
        text: c(0xcdd6f4),
        text_dim: c(0x9aa0b8),
        text_muted: c(0x6c7086),
        hover: c(0x252537),
        active: c(0x313244),
        selected: c(0x2a2b45),
        accent: c(0x89b4fa),
        on_accent: c(0x181825),
        success: c(0xa6e3a1),
        warning: c(0xf9e2af),
        danger: c(0xf38ba8),
        accents: ACCENTS_MOCHA.iter().copied().map(c).collect(),
        typography: Typography::default(),
    }
}

pub fn latte() -> Theme {
    Theme {
        name: "latte".into(),
        appearance: Some(Appearance::Light),
        background: c(0xeff1f5),
        surface: c(0xe6e9ef),
        elevated: c(0xffffff),
        sunken: c(0xdce0e8),
        border: c(0xbcc0cc),
        border_focus: c(0xacb0be),
        text: c(0x4c4f69),
        text_dim: c(0x6c6f85),
        text_muted: c(0x8c8fa1),
        hover: c(0xdce0e8),
        active: c(0xccd0da),
        selected: c(0xdce4f7),
        accent: c(0x1e66f5),
        on_accent: c(0xeff1f5),
        success: c(0x40a02b),
        warning: c(0xdf8e1d),
        danger: c(0xd20f39),
        accents: ACCENTS_LATTE.iter().copied().map(c).collect(),
        typography: Typography::default(),
    }
}

const ACCENTS_MOCHA: [u32; 8] = [
    0x89b4fa, 0xa6e3a1, 0xf9e2af, 0xfab387, 0xcba6f7, 0x94e2d5, 0xf5c2e7, 0xf38ba8,
];

const ACCENTS_LATTE: [u32; 8] = [
    0x1e66f5, 0x40a02b, 0xdf8e1d, 0xfe640b, 0x8839ef, 0x179299, 0xea76cb, 0xd20f39,
];

/// Two themes compile in so `theme = "latte"` works with no files on disk;
/// anything else is read from `~/.config/petunia/themes/<name>.toml`.
pub fn load(name: &str) -> (Theme, Option<String>) {
    match name {
        "" | "mocha" | "dark" => return (mocha(), None),
        "latte" | "light" => return (latte(), None),
        _ => {}
    }

    let path = super::themes_dir().join(format!("{name}.toml"));
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<Theme>(&contents) {
            Ok(mut theme) => {
                if theme.accents.is_empty() {
                    theme.accents = mocha().accents;
                }
                if theme.name.is_empty() {
                    theme.name = name.to_owned();
                }
                (theme, None)
            }
            Err(error) => (
                mocha(),
                Some(format!("theme {name}: {}", error.message().replace('\n', " "))),
            ),
        },
        Err(error) => (
            mocha(),
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
        assert!(!mocha().is_light());
        assert!(latte().is_light());
    }

    /// With no `appearance` key the background's luma has to decide, which is
    /// what every hand-written theme file relies on.
    #[test]
    fn appearance_falls_back_to_the_background_luma() {
        let dark = Theme {
            appearance: None,
            ..mocha()
        };
        let light = Theme {
            appearance: None,
            ..latte()
        };

        assert!(!dark.is_light());
        assert!(light.is_light());
    }

    #[test]
    fn both_built_in_themes_have_accents() {
        assert_eq!(mocha().accents.len(), 8);
        assert_eq!(latte().accents.len(), 8);
    }

    #[test]
    fn the_named_built_ins_load_without_a_file() {
        for name in ["", "mocha", "dark", "latte", "light"] {
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
        let theme = mocha();
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
            ..mocha()
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
        assert_eq!(theme.background, mocha().background);
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

        assert_eq!(theme.name, "petunia-dark");
        assert_eq!(theme.appearance, Some(Appearance::Dark));
        assert!(!theme.is_light());
        assert_eq!(theme.accents.len(), 8);
        assert_eq!(theme.typography.ui_size, 13.0);
    }
}
