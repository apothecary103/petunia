//! Writing `config.toml` back out.
//!
//! The file stays authoritative: the settings window edits it rather than
//! keeping a second copy of the truth somewhere, and the watcher then reloads it
//! exactly as if it had been edited by hand. That is also why this writes whole
//! documents rather than patching lines — a round trip through the same struct
//! the loader reads is the only way to be sure the two agree.
//!
//! Comments in a hand-edited file do not survive. Nothing here can promise they
//! would: a config written from a struct has no idea what was written around it.

use std::fmt::Write as _;

use super::{Config, GroupNotifications, Sort};
use crate::messages::{Density, Layout, Timestamps};

/// The whole file, as petunia would write it.
pub fn to_toml(config: &Config) -> String {
    let mut out = String::new();

    out.push_str("# Written by petunia's settings. Editing this by hand works\n");
    out.push_str("# too -- it is reloaded on save -- but comments added here are\n");
    out.push_str("# lost the next time settings writes it out.\n\n");

    let _ = writeln!(out, "theme = {:?}", config.theme);
    let _ = writeln!(out, "scale = {:?}", config.scale);

    let _ = writeln!(out, "\n[messages]");
    let _ = writeln!(out, "layout = {:?}", layout(config.messages.layout));
    let _ = writeln!(out, "density = {:?}", density(config.messages.density));
    let _ = writeln!(out, "timestamps = {:?}", timestamps(config.messages.timestamps));
    let _ = writeln!(out, "group_within = {}", config.messages.group_within);
    let _ = writeln!(out, "date_separators = {}", config.messages.date_separators);
    let _ = writeln!(out, "show_own_name = {}", config.messages.show_own_name);

    let _ = writeln!(out, "\n[sidebar]");
    let _ = writeln!(out, "width = {:?}", config.sidebar.width);
    let _ = writeln!(out, "sort = {:?}", sort(config.sidebar.sort));
    let _ = writeln!(out, "show_preview = {}", config.sidebar.show_preview);
    let _ = writeln!(out, "translucent = {}", config.sidebar.translucent);

    let _ = writeln!(out, "\n[media]");
    let media = &config.media;
    let _ = writeln!(out, "auto_download_images = {}", media.auto_download_images);
    let _ = writeln!(out, "auto_download_audio = {}", media.auto_download_audio);
    let _ = writeln!(out, "auto_download_video = {}", media.auto_download_video);
    let _ = writeln!(out, "auto_download_limit = {}", media.auto_download_limit);
    let _ = writeln!(out, "cache_limit = {}", media.cache_limit);
    let _ = writeln!(out, "image_max_width = {:?}", media.image_max_width);
    let _ = writeln!(out, "image_max_height = {:?}", media.image_max_height);

    let _ = writeln!(out, "\n[notifications]");
    let notifications = &config.notifications;
    let _ = writeln!(out, "enabled = {}", notifications.enabled);
    let _ = writeln!(out, "show_content = {}", notifications.show_content);
    let _ = writeln!(out, "show_sender = {}", notifications.show_sender);
    let _ = writeln!(out, "groups = {:?}", groups(notifications.groups));

    let _ = writeln!(out, "\n[keys]");
    for (chord, action) in config.keys.written() {
        let _ = writeln!(out, "{action} = {chord:?}");
    }

    out
}

/// Writes the file, creating the directory if this is the first time.
pub fn save(config: &Config) -> Result<(), std::io::Error> {
    let path = super::config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Written beside and renamed, so a crash never leaves a half-written config
    // that the next start would report as broken.
    let staged = path.with_extension("toml.part");
    std::fs::write(&staged, to_toml(config))?;
    std::fs::rename(&staged, &path)
}

fn layout(layout: Layout) -> &'static str {
    match layout {
        Layout::Standard => "standard",
        Layout::Compact => "compact",
        Layout::Bubbles => "bubbles",
    }
}

fn density(density: Density) -> &'static str {
    match density {
        Density::Compact => "compact",
        Density::Comfortable => "comfortable",
    }
}

fn timestamps(timestamps: Timestamps) -> &'static str {
    match timestamps {
        Timestamps::Always => "always",
        Timestamps::Hover => "hover",
        Timestamps::Never => "never",
    }
}

fn sort(sort: Sort) -> &'static str {
    match sort {
        Sort::Recent => "recent",
        Sort::Name => "name",
    }
}

fn groups(groups: GroupNotifications) -> &'static str {
    match groups {
        GroupNotifications::All => "all",
        GroupNotifications::Mentions => "mentions",
        GroupNotifications::None => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Keys, Preset};

    /// The round trip is the whole contract: what settings writes has to be
    /// what the loader reads, or the window and the file disagree about what is
    /// true and one of them silently wins.
    #[test]
    fn a_written_config_reads_back_the_same() {
        let config = Config {
            theme: "gruvbox-dark".into(),
            scale: 1.25,
            messages: crate::Messages {
                layout: Layout::Bubbles,
                density: Density::Compact,
                timestamps: Timestamps::Hover,
                group_within: 120,
                show_own_name: true,
                ..Default::default()
            },
            sidebar: crate::Sidebar {
                width: 320.0,
                sort: Sort::Name,
                show_preview: false,
                translucent: false,
            },
            media: crate::Media {
                auto_download_video: false,
                cache_limit: 512,
                ..Default::default()
            },
            notifications: crate::Notifications {
                groups: GroupNotifications::Mentions,
                ..Default::default()
            },
            ..Default::default()
        };

        let read: Config = toml::from_str(&to_toml(&config)).expect("written config parses");

        assert_eq!(read.theme, config.theme);
        assert_eq!(read.scale, config.scale);
        assert_eq!(read.messages.layout, config.messages.layout);
        assert_eq!(read.messages.density, config.messages.density);
        assert_eq!(read.messages.timestamps, config.messages.timestamps);
        assert_eq!(read.messages.group_within, config.messages.group_within);
        assert_eq!(read.messages.show_own_name, config.messages.show_own_name);
        assert_eq!(read.sidebar.width, config.sidebar.width);
        assert_eq!(read.sidebar.sort, config.sidebar.sort);
        assert_eq!(read.sidebar.show_preview, config.sidebar.show_preview);
        assert_eq!(read.sidebar.translucent, config.sidebar.translucent);
        assert_eq!(read.media.auto_download_video, config.media.auto_download_video);
        assert_eq!(read.media.cache_limit, config.media.cache_limit);
        assert_eq!(read.notifications.groups, config.notifications.groups);
    }

    #[test]
    fn the_defaults_round_trip() {
        let read: Config = toml::from_str(&to_toml(&Config::default())).unwrap();

        assert_eq!(read.scale, 1.0);
        assert_eq!(read.media.image_max_width, 400.0);
    }

    /// `bindings` walks a map, so the order means nothing and the set means
    /// everything.
    fn keymap(config: &Config) -> Vec<(String, crate::keys::Action)> {
        let mut bindings = config.keys.bindings();
        bindings.sort_by(|a, b| a.0.cmp(&b.0));
        bindings
    }

    /// Keybindings have to survive the trip too, or opening settings once
    /// silently resets everyone's keymap.
    #[test]
    fn keybindings_round_trip() {
        let config = Config {
            keys: Keys::preset(Preset::Emacs),
            ..Default::default()
        };

        let read: Config = toml::from_str(&to_toml(&config)).unwrap();

        assert_eq!(keymap(&read), keymap(&config));
    }

    #[test]
    fn an_edited_binding_round_trips() {
        let config = Config {
            keys: toml::from_str(r#"help = "cmd+?""#).unwrap(),
            ..Default::default()
        };

        let read: Config = toml::from_str(&to_toml(&config)).unwrap();

        assert_eq!(keymap(&read), keymap(&config));
    }

    /// A preset chosen in settings is written as the bindings it produced, not
    /// as its name -- so a later change to what "emacs" means cannot silently
    /// move someone's keys.
    #[test]
    fn a_preset_is_written_as_its_bindings() {
        let config = Config {
            keys: Keys::preset(Preset::Vim),
            ..Default::default()
        };

        let written = to_toml(&config);

        assert!(!written.contains("preset"), "{written}");
        assert!(written.contains("scroll-down = \"ctrl+d\""), "{written}");
    }
}
