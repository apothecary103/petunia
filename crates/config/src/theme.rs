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

    /// A fixed palette of sender colours, for a theme that wants to name them.
    /// Left out -- which is what petunia's own themes do -- and a colour is
    /// generated per person instead, which is the only way to have as many
    /// distinguishable ones as a group has members.
    #[serde(deserialize_with = "hex_list")]
    pub accents: Vec<Hsla>,

    /// What a code block is coloured with, by tree-sitter capture name. Zed's
    /// own palette, carried through: colouring code is the same job in a chat
    /// window as in an editor, and a second palette would be a second thing to
    /// keep in step.
    #[serde(default)]
    pub syntax: std::collections::BTreeMap<String, String>,

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
    /// What maths is set in. Nothing else uses it: a serif in the middle of an
    /// interface built on one sans face would be a second voice.
    pub serif: String,
    pub ui_size: f32,
    pub message_size: f32,
    pub line_height: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            family: system_ui().into(),
            mono: system_mono().into(),
            serif: system_serif().into(),
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

/// Menlo rather than SF Mono on macOS: SF Mono ships with the developer tools
/// and its public family name does not always resolve, and a code block that
/// silently falls back to the interface font is not a code block.
const fn system_mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "monospace"
    }
}

/// What maths is set in. Every typesetter sets it in a serif, and the interface
/// font it was falling back to made an integral sign look like a stray glyph
/// rather than an operator.
///
/// Times New Roman rather than the newer system serifs: it is on macOS and on
/// Windows, it has the whole of the mathematical operator block, and its italic
/// is a real italic rather than a slanted roman -- which is what a variable set
/// in it depends on.
const fn system_serif() -> &'static str {
    if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
        "Times New Roman"
    } else {
        "serif"
    }
}

impl Default for Theme {
    fn default() -> Self {
        dark()
    }
}

impl Theme {
    /// The palette a code block is coloured with, in the shape the widget
    /// library's highlighter reads -- which is Zed's own theme JSON, so this is
    /// a translation rather than a mapping.
    pub fn highlights(&self) -> gpui_component::highlighter::HighlightTheme {
        let syntax: serde_json::Map<String, serde_json::Value> = self
            .syntax
            .iter()
            .map(|(name, colour)| {
                (
                    name.clone(),
                    serde_json::json!({ "color": colour }),
                )
            })
            .collect();

        let described = serde_json::json!({
            "name": self.name,
            "appearance": if self.is_light() { "light" } else { "dark" },
            "style": {
                "editor.background": rgb(self.sunken),
                "editor.foreground": rgb(self.text),
                // The editor's own chrome, which the code editor takes from here
                // and nowhere else. A code block in a message paints its own.
                "editor.gutter.background": rgb(self.sunken),
                "editor.active_line.background": rgb(self.hover),
                "editor.invisible": rgb(self.text_muted),
                "syntax": syntax,
            },
        });

        serde_json::from_value(described).unwrap_or_else(|_| match self.is_light() {
            true => (*gpui_component::highlighter::HighlightTheme::default_light()).clone(),
            false => (*gpui_component::highlighter::HighlightTheme::default_dark()).clone(),
        })
    }

    /// A stable per-sender colour, so the same person is the same colour in
    /// every thread and across restarts.
    ///
    /// Generated rather than picked out of a list of eight. A list that short
    /// puts two people in a group of ten in the same colour more often than not,
    /// and a list of eight *muted* colours -- which is what a neutral theme
    /// wants -- is eight shades of the same idea. A hue taken from the whole
    /// wheel gives everyone their own, and holding saturation and lightness to
    /// the band below keeps them all legible on the same background.
    pub fn accent_for(&self, seed: &[u8]) -> Hsla {
        let hash = hash(seed);

        if !self.accents.is_empty() {
            return self.accents[(hash % self.accents.len() as u64) as usize];
        }
        generated(hash, self.is_light())
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

/// FNV-1a. A sum of the bytes -- which is what this used to be -- collides on
/// every reordering of them, and a uuid is sixteen bytes in an order; worse, a
/// sum mod eight only ever reads the bottom three bits of it.
fn hash(seed: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// How many places on the wheel a sender colour can land, and how many tones at
/// each. Quantised rather than continuous: a hue taken straight from a hash puts
/// two of ten people within a few degrees of each other more often than not,
/// which is the same complaint as a palette of eight. Snapped to a step, two
/// colours are either the same or a clear fifteen degrees apart, and the tone
/// tells the ones that landed together apart.
const HUES: u64 = 24;
const TONES: u64 = 4;

/// A sender colour from a hash: a place on the wheel, and one of four tones
/// there.
///
/// The lightness band is tilted rather than flat. Yellows and greens read far
/// brighter than blues and violets at the same nominal lightness, so a flat band
/// gives a conversation where half the names shout.
fn generated(hash: u64, light: bool) -> Hsla {
    let hue = ((hash >> 40) % HUES) as f32 / HUES as f32;
    let tone = ((hash >> 12) % TONES) as f32 / (TONES - 1) as f32;
    // 1.0 at yellow-green, falling to 0 by cyan one way and orange the other.
    let warm = 1.0 - ((hue - 0.22).abs() * 5.0).min(1.0);

    let (saturation, lightness) = match light {
        true => (0.60 + 0.12 * tone, 0.40 - 0.08 * warm + 0.05 * tone),
        false => (0.52 + 0.16 * tone, 0.72 - 0.07 * warm + 0.08 * tone),
    };

    Hsla {
        h: hue,
        s: saturation,
        l: lightness,
        a: 1.0,
    }
}

fn c(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

/// Back to `#rrggbb`, for the one place a colour has to leave as text.
fn rgb(colour: Hsla) -> String {
    let Rgba { r, g, b, .. } = colour.into();
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
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
        // Empty: sender colours are generated per person. See `accent_for`.
        accents: Vec::new(),
        syntax: syntax(ONE_DARK_SYNTAX),
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
        // Darker than the dark theme's greys are light, because a light theme's
        // quiet text has to hold up over a *filled* row as well as over the
        // background: the greys these were mirrored from put the selected
        // conversation's preview at barely two to one against its own fill.
        text_dim: c(0x4e4e4e),
        text_muted: c(0x6f6f6f),
        hover: c(0xe2e2e2),
        active: c(0xd4d4d4),
        selected: c(0xdcdcdc),
        accent: c(0x1b1b1b),
        on_accent: c(0xf5f5f5),
        success: c(0x1f7a45),
        warning: c(0x8a6100),
        danger: c(0xbb3a44),
        accents: Vec::new(),
        syntax: syntax(ONE_LIGHT_SYNTAX),
        typography: Typography::default(),
    }
}

/// The themes that need no files on disk: petunia's own two, Zed's -- converted
/// by `script/zed-themes.py` -- and Signal's own dark palette, which is written
/// by hand because there is no Zed theme behind it.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("one-dark", include_str!("../../../themes/one-dark.toml")),
    ("one-light", include_str!("../../../themes/one-light.toml")),
    ("ayu-dark", include_str!("../../../themes/ayu-dark.toml")),
    ("ayu-mirage", include_str!("../../../themes/ayu-mirage.toml")),
    ("ayu-light", include_str!("../../../themes/ayu-light.toml")),
    ("gruvbox-dark", include_str!("../../../themes/gruvbox-dark.toml")),
    ("gruvbox-dark-hard", include_str!("../../../themes/gruvbox-dark-hard.toml")),
    ("gruvbox-dark-soft", include_str!("../../../themes/gruvbox-dark-soft.toml")),
    ("gruvbox-light", include_str!("../../../themes/gruvbox-light.toml")),
    ("gruvbox-light-hard", include_str!("../../../themes/gruvbox-light-hard.toml")),
    ("gruvbox-light-soft", include_str!("../../../themes/gruvbox-light-soft.toml")),
    ("signal-dark", include_str!("../../../themes/signal-dark.toml")),
];

/// Every theme that can be chosen without writing a file, for the settings
/// window and for `--help`-style listing.
pub fn available() -> Vec<String> {
    let mut names = vec!["dark".to_string(), "light".to_string()];
    names.extend(BUILT_IN.iter().map(|(name, _)| (*name).to_string()));

    if let Ok(entries) = std::fs::read_dir(super::themes_dir()) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "toml")
                && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            {
                names.push(stem.to_owned());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Petunia's own two and Zed's compile in, so `theme = "one-dark"` works with
/// nothing installed; anything else is read from
/// `~/.config/petunia/themes/<name>.toml`, which also overrides a built-in of
/// the same name.
pub fn load(name: &str) -> (Theme, Option<String>) {
    match name {
        "" | "dark" => return (dark(), None),
        "light" => return (light(), None),
        _ => {}
    }

    let path = super::themes_dir().join(format!("{name}.toml"));
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => match BUILT_IN.iter().find(|(built, _)| *built == name) {
            Some((_, contents)) => (*contents).to_string(),
            // Only worth reporting when there was nowhere else to look.
            None => {
                return (
                    dark(),
                    Some(format!("theme {name} ({}): {error}", path.display())),
                );
            }
        },
    };

    match toml::from_str::<Theme>(&contents) {
        Ok(mut theme) => {
            if theme.name.is_empty() {
                theme.name = name.to_owned();
            }
            (theme, None)
        }
        Err(error) => (
            dark(),
            Some(format!("theme {name}: {}", error.message().replace('\n', " "))),
        ),
    }
}

fn syntax(entries: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    entries
        .iter()
        .map(|(name, colour)| ((*name).to_string(), (*colour).to_string()))
        .collect()
}

/// Petunia's own dark and light are neutral greys with no syntax palette of
/// their own, and code with no colour in it is worse than code in someone
/// else's colours. These are One Dark's and One Light's.
const ONE_DARK_SYNTAX: &[(&str, &str)] = &[
    ("attribute", "#74ade8"),
    ("boolean", "#bf956a"),
    ("comment", "#5d636f"),
    ("comment.doc", "#878e98"),
    ("constant", "#dfc184"),
    ("constructor", "#73ade9"),
    ("emphasis", "#74ade8"),
    ("emphasis.strong", "#bf956a"),
    ("function", "#73ade9"),
    ("keyword", "#b477cf"),
    ("label", "#74ade8"),
    ("link_text", "#73ade9"),
    ("link_uri", "#6eb4bf"),
    ("number", "#bf956a"),
    ("operator", "#6eb4bf"),
    ("primary", "#acb2be"),
    ("property", "#d07277"),
    ("punctuation", "#acb2be"),
    ("punctuation.bracket", "#b2b9c6"),
    ("punctuation.delimiter", "#b2b9c6"),
    ("string", "#a1c181"),
    ("string.escape", "#878e98"),
    ("string.regex", "#bf956a"),
    ("string.special", "#bf956a"),
    ("tag", "#74ade8"),
    ("title", "#d07277"),
    ("type", "#6eb4bf"),
    ("variable", "#acb2be"),
    ("variable.parameter", "#d07277"),
];

const ONE_LIGHT_SYNTAX: &[(&str, &str)] = &[
    ("attribute", "#5c78e2"),
    ("boolean", "#ad6e26"),
    ("comment", "#a2a3a7"),
    ("comment.doc", "#646466"),
    ("constant", "#669f59"),
    ("constructor", "#5c79e2"),
    ("emphasis", "#5c78e2"),
    ("emphasis.strong", "#ad6e26"),
    ("function", "#5c79e2"),
    ("keyword", "#a449ab"),
    ("label", "#5c78e2"),
    ("link_text", "#5c79e2"),
    ("link_uri", "#3882b7"),
    ("number", "#ad6e25"),
    ("operator", "#3882b7"),
    ("primary", "#383a41"),
    ("property", "#d3604f"),
    ("punctuation", "#383a41"),
    ("punctuation.bracket", "#4d4f52"),
    ("punctuation.delimiter", "#4d4f52"),
    ("string", "#659f58"),
    ("string.escape", "#646466"),
    ("string.regex", "#ad6e26"),
    ("string.special", "#ad6e26"),
    ("tag", "#5c78e2"),
    ("title", "#d3604f"),
    ("type", "#3882b7"),
    ("variable", "#383a41"),
    ("variable.parameter", "#d3604f"),
];

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

    /// Petunia's own two name no palette, so senders are coloured by generation.
    /// A fixed list of eight in a neutral theme is eight shades of one idea.
    #[test]
    fn the_built_in_themes_generate_their_sender_colours() {
        assert!(dark().accents.is_empty());
        assert!(light().accents.is_empty());
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
    fn a_sender_colour_is_stable() {
        let theme = dark();
        let seed = [1u8, 2, 3, 4];

        assert_eq!(theme.accent_for(&seed), theme.accent_for(&seed));
    }

    /// A theme that names a palette gets that palette, and the hash rather than
    /// a byte sum decides which entry -- so two names that happen to share their
    /// bytes in a different order do not share a colour.
    #[test]
    fn a_named_palette_is_used_as_given() {
        let theme = Theme {
            accents: vec![c(0x111111), c(0x222222)],
            ..dark()
        };

        assert!(theme.accents.contains(&theme.accent_for(&[9, 9, 9])));
    }

    /// Sixteen bytes differing in one of them, which is the hardest case: a byte
    /// sum would put ten of these in three buckets.
    fn seeds(count: u8) -> Vec<[u8; 16]> {
        (0..count)
            .map(|index| {
                let mut seed = [0x5eu8; 16];
                seed[7] = index;
                seed
            })
            .collect()
    }

    /// The whole point. A group has to read as a group of individuals, so no two
    /// colours may be *nearly* the same: either they are the same place on the
    /// wheel or they are a clear distance apart.
    #[test]
    fn two_sender_colours_are_never_almost_the_same() {
        let theme = dark();
        let hues: Vec<f32> = seeds(64)
            .iter()
            .map(|seed| theme.accent_for(seed).h)
            .collect();

        for (at, hue) in hues.iter().enumerate() {
            for other in &hues[at + 1..] {
                let apart = (hue - other).abs();
                let apart = apart.min(1.0 - apart) * 360.0;
                assert!(
                    apart == 0.0 || apart >= 14.0,
                    "{hue} and {other} are {apart} degrees apart"
                );
            }
        }
    }

    /// And there have to be enough of them to go round. A palette of eight ran
    /// out at the ninth person; this is a hue step times a tone, so a group is
    /// nearly always all-different and two people sharing is the exception.
    #[test]
    fn there_are_far_more_colours_than_a_group_has_members() {
        let theme = dark();
        let mut colours: Vec<String> = seeds(255)
            .iter()
            .map(|seed| rgb(theme.accent_for(seed)))
            .collect();
        colours.sort();
        colours.dedup();

        assert!(colours.len() > 60, "only {} colours", colours.len());
    }

    /// Every generated colour has to be legible on the background it is drawn
    /// on, or the spread is bought with names nobody can read.
    #[test]
    fn generated_colours_stay_inside_the_legible_band() {
        for light in [true, false] {
            for seed in 0..256u64 {
                let colour = generated(hash(&seed.to_le_bytes()), light);

                assert!((0.0..=1.0).contains(&colour.h));
                match light {
                    true => assert!(colour.l < 0.5, "{colour:?}"),
                    false => assert!(colour.l > 0.6, "{colour:?}"),
                }
            }
        }
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
        let example = include_str!("../../../examples/themes.example.toml");

        let theme: Theme = toml::from_str(example).expect("example theme parses");

        assert_eq!(theme.name, "my-theme");
        assert_eq!(theme.appearance, Some(Appearance::Dark));
        assert!(!theme.is_light());
        assert_eq!(theme.accents.len(), 8);
        assert_eq!(theme.typography.ui_size, 13.0);
    }

    /// Every shipped theme has to parse, or `theme = "one-dark"` reports a
    /// problem and silently falls back.
    #[test]
    fn every_built_in_theme_parses() {
        for (name, contents) in BUILT_IN {
            let theme: Theme = toml::from_str(contents)
                .unwrap_or_else(|error| panic!("{name}: {error}"));

            assert!(!theme.name.is_empty(), "{name}");
            assert!(!theme.accents.is_empty(), "{name} has no sender colours");
            assert!(!theme.syntax.is_empty(), "{name} has no syntax palette");
        }
    }

    /// The name in the file is the one shown; the key is what selects it. A
    /// light theme that claims to be dark would derive every widget shade wrong.
    #[test]
    fn the_built_in_themes_know_which_they_are() {
        for (name, contents) in BUILT_IN {
            let theme: Theme = toml::from_str(contents).unwrap();
            assert_eq!(theme.is_light(), name.contains("light"), "{name}");
        }
    }

    #[test]
    fn a_built_in_theme_loads_without_a_file() {
        let (theme, error) = load("one-dark");

        assert!(error.is_none(), "{error:?}");
        assert_eq!(theme.name, "One Dark");
    }

    #[test]
    fn every_built_in_is_offered() {
        let names = available();

        assert!(names.contains(&"dark".to_string()));
        assert!(names.contains(&"one-dark".to_string()));
        assert!(names.contains(&"gruvbox-light-soft".to_string()));
    }

    /// The highlighter reads Zed's own JSON shape, so this is the one place a
    /// translation could silently produce an empty palette.
    #[test]
    fn a_theme_translates_into_a_highlight_palette() {
        let highlights = dark().highlights();

        assert!(highlights.style("keyword").is_some());
        assert!(highlights.style("string").is_some());
    }

    /// The code editor takes its chrome from the same place as its syntax, so a
    /// key that fails to parse leaves it coloured by the library's default.
    #[test]
    fn the_highlight_palette_carries_the_editor_chrome() {
        let theme = dark();
        let style = theme.highlights().style;

        assert_eq!(style.editor_background, Some(theme.sunken));
        assert_eq!(style.editor_foreground, Some(theme.text));
        assert_eq!(style.editor_gutter_background, Some(theme.sunken));
        assert_eq!(style.editor_active_line, Some(theme.hover));
    }

    /// Petunia's own two are neutral greys with no palette of their own, and
    /// code with no colour is worse than code in someone else's colours.
    #[test]
    fn the_neutral_themes_still_colour_code() {
        assert!(!dark().syntax.is_empty());
        assert!(!light().syntax.is_empty());
    }
}
