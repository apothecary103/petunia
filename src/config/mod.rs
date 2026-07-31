pub mod keys;
pub mod messages;
pub mod theme;
pub mod watch;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

pub use keys::{Action, Keys};
pub use messages::Messages;
pub use theme::Theme;

/// Hand-edited preferences. Every section defaults, so an absent or partial file
/// is as valid as a complete one.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub theme: String,
    /// Multiplies every length in the interface. The one knob for "everything is
    /// too small", since gpui sizes elements in logical pixels and the display's
    /// own scale factor is already applied underneath.
    pub scale: f32,
    pub messages: Messages,
    pub sidebar: Sidebar,
    pub media: Media,
    pub notifications: Notifications,
    pub keys: Keys,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sidebar {
    pub width: f32,
    pub sort: Sort,
    pub show_preview: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sort {
    Recent,
    Name,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Media {
    /// Fetched without asking, because CDN entries expire in weeks and a
    /// pointer that outlives its bytes is unrecoverable.
    pub auto_download_images: bool,
    pub auto_download_audio: bool,
    pub auto_download_video: bool,
    /// Megabytes. Anything larger waits to be asked for.
    pub auto_download_limit: u32,
    /// Megabytes of cached media to keep before the oldest is dropped.
    pub cache_limit: u32,
    /// Inline media is scaled down to fit inside both of these, at its own aspect
    /// ratio. Without a width cap a wide screenshot lays out thousands of units
    /// across, because an image's natural size is its pixel size.
    pub image_max_width: f32,
    pub image_max_height: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Notifications {
    pub enabled: bool,
    pub show_content: bool,
    pub show_sender: bool,
    pub groups: GroupNotifications,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupNotifications {
    All,
    Mentions,
    None,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: String::default(),
            scale: 1.0,
            messages: Messages::default(),
            sidebar: Sidebar::default(),
            media: Media::default(),
            notifications: Notifications::default(),
            keys: Keys::default(),
        }
    }
}

impl Config {
    /// Clamped, because a hand-edited `scale = 40` would leave a window with no
    /// reachable controls and no way to edit the file from inside it.
    pub fn scale(&self) -> f32 {
        if self.scale.is_finite() {
            self.scale.clamp(0.5, 3.0)
        } else {
            1.0
        }
    }
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            width: 240.0,
            sort: Sort::Recent,
            show_preview: true,
        }
    }
}

impl Default for Media {
    fn default() -> Self {
        Self {
            auto_download_images: true,
            auto_download_audio: true,
            auto_download_video: false,
            auto_download_limit: 8,
            cache_limit: 2048,
            image_max_width: 400.0,
            image_max_height: 300.0,
        }
    }
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: true,
            show_content: true,
            show_sender: true,
            groups: GroupNotifications::All,
        }
    }
}

/// What `load` produced. Never an error: a broken config must not stop the app
/// from starting, so problems are reported and the defaults stand.
pub struct Loaded {
    pub config: Config,
    pub theme: Arc<Theme>,
    pub errors: Vec<String>,
}

pub fn load() -> Loaded {
    let mut errors = Vec::new();
    let config = match std::fs::read_to_string(config_path()) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(config) => config,
            Err(error) => {
                errors.push(describe(&error));
                Config::default()
            }
        },
        // No file at all is the normal first run, not a problem.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(error) => {
            errors.push(format!("could not read config.toml: {error}"));
            Config::default()
        }
    };

    let (theme, theme_error) = theme::load(&config.theme);
    errors.extend(theme_error);

    Loaded {
        config,
        theme: Arc::new(theme),
        errors,
    }
}

/// Points at the line, because "invalid type" with no location is not actionable.
fn describe(error: &toml::de::Error) -> String {
    match error.span() {
        Some(_) => format!("config.toml: {}", error.message().replace('\n', " ")),
        None => format!("config.toml: {error}"),
    }
}

pub fn dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("petunia")
}

pub fn config_path() -> PathBuf {
    dir().join("config.toml")
}

pub fn themes_dir() -> PathBuf {
    dir().join("themes")
}

pub fn store_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("petunia")
        .join("petunia.db3")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("petunia")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_config_is_all_defaults() {
        let config: Config = toml::from_str("").unwrap();

        assert_eq!(config.sidebar.width, 240.0);
        assert!(config.media.auto_download_images);
        assert_eq!(config.messages.density, messages::Density::Comfortable);
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let config: Config = toml::from_str("[sidebar]\nwidth = 300.0").unwrap();

        assert_eq!(config.sidebar.width, 300.0);
        assert_eq!(config.sidebar.sort, Sort::Recent);
        assert!(config.sidebar.show_preview);
    }

    /// A typo must be reported rather than silently ignored, or the user edits a
    /// key that does nothing and concludes the app is broken.
    #[test]
    fn an_unknown_key_is_an_error() {
        let error = toml::from_str::<Config>("[sidebar]\nwdith = 300.0").unwrap_err();

        assert!(error.message().contains("unknown field"), "{error}");
    }

    #[test]
    fn enums_are_written_in_kebab_case() {
        let config: Config =
            toml::from_str("[notifications]\ngroups = \"mentions\"").unwrap();

        assert_eq!(config.notifications.groups, GroupNotifications::Mentions);
    }

    /// The shipped example is the schema's documentation, so it must parse and
    /// must not drift from the struct definitions.
    #[test]
    fn the_example_config_parses() {
        let example = include_str!("../../config.example.toml");

        let config: Config = toml::from_str(example).expect("example config parses");

        assert_eq!(config.theme, "mocha");
        assert_eq!(config.scale, 1.0);
        assert_eq!(config.messages.density, messages::Density::Comfortable);
        assert_eq!(config.sidebar.width, 240.0);
        assert_eq!(config.media.image_max_width, 400.0);
    }

    /// A hand-edited scale must not be able to produce a window with no reachable
    /// controls, since the only way to fix it is a control inside that window.
    #[test]
    fn scale_is_clamped_to_something_usable() {
        let huge: Config = toml::from_str("scale = 40.0").unwrap();
        let tiny: Config = toml::from_str("scale = 0.0").unwrap();
        let broken: Config = toml::from_str("scale = nan").unwrap();

        assert_eq!(huge.scale(), 3.0);
        assert_eq!(tiny.scale(), 0.5);
        assert_eq!(broken.scale(), 1.0);
    }

    /// Every commented-out keybind in the example must name a real action and a
    /// parseable chord, or the documentation lies.
    #[test]
    fn the_example_keybinds_are_all_valid() {
        let example = include_str!("../../config.example.toml");
        let bindings: String = example
            .lines()
            .skip_while(|line| !line.starts_with("[keys]"))
            .filter_map(|line| line.strip_prefix("# "))
            .filter(|line| line.contains('='))
            .map(|line| format!("{line}\n"))
            .collect();

        assert!(!bindings.is_empty(), "no example keybinds found");
        toml::from_str::<keys::Keys>(&bindings).expect("example keybinds are valid");
    }

    #[test]
    fn density_and_timestamps_parse() {
        let config: Config =
            toml::from_str("[messages]\ndensity = \"compact\"\ntimestamps = \"hover\"").unwrap();

        assert_eq!(config.messages.density, messages::Density::Compact);
        assert_eq!(config.messages.timestamps, messages::Timestamps::Hover);
    }
}
