use iced::Color;
use serde::{Deserialize, Deserializer};

/// Petunia's palette. Wider than `iced::theme::Palette`, which carries only six
/// colours, so it lives beside `iced::Theme` rather than inside it.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    #[serde(deserialize_with = "hex")]
    pub background: Color,
    #[serde(deserialize_with = "hex")]
    pub surface: Color,
    #[serde(deserialize_with = "hex")]
    pub sunken: Color,
    #[serde(deserialize_with = "hex")]
    pub border: Color,
    #[serde(deserialize_with = "hex")]
    pub text: Color,
    #[serde(deserialize_with = "hex")]
    pub dim: Color,
    #[serde(deserialize_with = "hex")]
    pub muted: Color,
    #[serde(deserialize_with = "hex")]
    pub accent: Color,
    #[serde(deserialize_with = "hex")]
    pub on_accent: Color,
    #[serde(deserialize_with = "hex")]
    pub success: Color,
    #[serde(deserialize_with = "hex")]
    pub warning: Color,
    #[serde(deserialize_with = "hex")]
    pub danger: Color,
    /// A `Vec` so a theme picks its own count; `accent_for` is modular.
    #[serde(deserialize_with = "hex_list")]
    pub accents: Vec<Color>,
}

impl Default for Colors {
    fn default() -> Self {
        MOCHA
    }
}

impl Colors {
    /// A stable per-sender colour, so the same person is the same colour in
    /// every thread and across restarts.
    pub fn accent_for(&self, seed: &[u8]) -> Color {
        if self.accents.is_empty() {
            return self.accent;
        }
        let sum: usize = seed.iter().map(|byte| *byte as usize).sum();
        self.accents[sum % self.accents.len()]
    }

    /// Whether this is a light theme, which decides how iced's built-in widgets
    /// derive their own shades.
    pub fn is_light(&self) -> bool {
        let Color { r, g, b, .. } = self.background;
        // Rec. 601 luma: close enough to perceptual for a binary decision.
        0.299 * r + 0.587 * g + 0.114 * b > 0.5
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

pub const MOCHA: Colors = Colors {
    background: rgb(0x18, 0x18, 0x25),
    surface: rgb(0x1e, 0x1e, 0x2e),
    sunken: rgb(0x11, 0x11, 0x1b),
    border: rgb(0x31, 0x32, 0x44),
    text: rgb(0xcd, 0xd6, 0xf4),
    dim: rgb(0x9a, 0xa0, 0xb8),
    muted: rgb(0x6c, 0x70, 0x86),
    accent: rgb(0x89, 0xb4, 0xfa),
    on_accent: rgb(0x18, 0x18, 0x25),
    success: rgb(0xa6, 0xe3, 0xa1),
    warning: rgb(0xf9, 0xe2, 0xaf),
    danger: rgb(0xf3, 0x8b, 0xa8),
    accents: Vec::new(),
};

pub const LATTE: Colors = Colors {
    background: rgb(0xef, 0xf1, 0xf5),
    surface: rgb(0xe6, 0xe9, 0xef),
    sunken: rgb(0xdc, 0xe0, 0xe8),
    border: rgb(0xbc, 0xc0, 0xcc),
    text: rgb(0x4c, 0x4f, 0x69),
    dim: rgb(0x6c, 0x6f, 0x85),
    muted: rgb(0x8c, 0x8f, 0xa1),
    accent: rgb(0x1e, 0x66, 0xf5),
    on_accent: rgb(0xef, 0xf1, 0xf5),
    success: rgb(0x40, 0xa0, 0x2b),
    warning: rgb(0xdf, 0x8e, 0x1d),
    danger: rgb(0xd2, 0x0f, 0x39),
    accents: Vec::new(),
};

/// `Vec` cannot be built in a `const`, so the built-in themes carry empty accent
/// lists and get them filled in here.
fn accents(colors: Colors, accents: &[Color]) -> Colors {
    Colors {
        accents: accents.to_vec(),
        ..colors
    }
}

const MOCHA_ACCENTS: [Color; 8] = [
    rgb(0x89, 0xb4, 0xfa),
    rgb(0xa6, 0xe3, 0xa1),
    rgb(0xf9, 0xe2, 0xaf),
    rgb(0xfa, 0xb3, 0x87),
    rgb(0xcb, 0xa6, 0xf7),
    rgb(0x94, 0xe2, 0xd5),
    rgb(0xf5, 0xc2, 0xe7),
    rgb(0xf3, 0x8b, 0xa8),
];

const LATTE_ACCENTS: [Color; 8] = [
    rgb(0x1e, 0x66, 0xf5),
    rgb(0x40, 0xa0, 0x2b),
    rgb(0xdf, 0x8e, 0x1d),
    rgb(0xfe, 0x64, 0x0b),
    rgb(0x88, 0x39, 0xef),
    rgb(0x17, 0x92, 0x99),
    rgb(0xea, 0x76, 0xcb),
    rgb(0xd2, 0x0f, 0x39),
];

pub fn mocha() -> Colors {
    accents(MOCHA, &MOCHA_ACCENTS)
}

pub fn latte() -> Colors {
    accents(LATTE, &LATTE_ACCENTS)
}

/// Two themes compile in so `theme = "latte"` works with no files on disk;
/// anything else is read from `~/.config/petunia/themes/<name>.toml`.
pub fn load(name: &str) -> (Colors, Option<String>) {
    match name {
        "" | "mocha" | "dark" => return (mocha(), None),
        "latte" | "light" => return (latte(), None),
        _ => {}
    }

    let path = super::themes_dir().join(format!("{name}.toml"));
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<Colors>(&contents) {
            Ok(mut colors) => {
                if colors.accents.is_empty() {
                    colors.accents = MOCHA_ACCENTS.to_vec();
                }
                (colors, None)
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

fn hex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Color, D::Error> {
    let raw = String::deserialize(deserializer)?;
    parse_hex(&raw).ok_or_else(|| serde::de::Error::custom(format!("not a #rrggbb colour: {raw}")))
}

fn hex_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Color>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|raw| {
            parse_hex(&raw)
                .ok_or_else(|| serde::de::Error::custom(format!("not a #rrggbb colour: {raw}")))
        })
        .collect()
}

/// Accepts `#rgb`, `#rrggbb` and `#rrggbbaa`, with or without the hash.
pub fn parse_hex(raw: &str) -> Option<Color> {
    let digits = raw.trim().trim_start_matches('#');
    let byte = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();

    match digits.len() {
        3 => {
            let nibble = |at: usize| {
                let value = u8::from_str_radix(digits.get(at..at + 1)?, 16).ok()?;
                Some(value * 17)
            };
            Some(Color::from_rgb8(nibble(0)?, nibble(1)?, nibble(2)?))
        }
        6 => Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color::from_rgba8(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)? as f32 / 255.0,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_three_hex_lengths() {
        assert_eq!(parse_hex("#fff"), Some(Color::from_rgb8(255, 255, 255)));
        assert_eq!(parse_hex("#ff8800"), Some(Color::from_rgb8(255, 136, 0)));
        assert_eq!(parse_hex("112233"), Some(Color::from_rgb8(17, 34, 51)));
    }

    #[test]
    fn an_alpha_channel_is_honoured() {
        let color = parse_hex("#00000080").unwrap();

        assert!((color.a - 128.0 / 255.0).abs() < f32::EPSILON);
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
        let (colors, error) = load("no-such-theme-exists-here");

        assert!(error.is_some());
        assert!(!colors.is_light());
    }

    #[test]
    fn a_sender_colour_is_stable_and_in_range() {
        let colors = mocha();
        let seed = [1u8, 2, 3, 4];

        assert_eq!(colors.accent_for(&seed), colors.accent_for(&seed));
        assert!(colors.accents.contains(&colors.accent_for(&seed)));
    }

    /// A theme file that lists no accents must still colour senders, so the
    /// modular index cannot divide by zero.
    #[test]
    fn no_accents_falls_back_to_the_single_accent() {
        let colors = Colors {
            accents: Vec::new(),
            ..mocha()
        };

        assert_eq!(colors.accent_for(&[7]), colors.accent);
    }

    #[test]
    fn a_theme_file_round_trips_through_toml() {
        let toml = r##"
            background = "#101010"
            surface = "#202020"
            sunken = "#000000"
            border = "#303030"
            text = "#eeeeee"
            dim = "#aaaaaa"
            muted = "#777777"
            accent = "#00aaff"
            on_accent = "#000000"
            success = "#00ff00"
            warning = "#ffff00"
            danger = "#ff0000"
            accents = ["#00aaff", "#ff00aa"]
        "##;

        let colors: Colors = toml::from_str(toml).unwrap();

        assert_eq!(colors.accents.len(), 2);
        assert_eq!(colors.accent, Color::from_rgb8(0, 0xaa, 0xff));
    }

    #[test]
    fn a_theme_file_may_override_only_what_it_cares_about() {
        let colors: Colors = toml::from_str(r##"accent = "#ff0000""##).unwrap();

        assert_eq!(colors.accent, Color::from_rgb8(255, 0, 0));
        assert_eq!(colors.background, MOCHA.background);
    }

    #[test]
    fn a_bad_colour_in_a_theme_file_is_an_error() {
        assert!(toml::from_str::<Colors>(r#"accent = "not-a-colour""#).is_err());
    }
}
