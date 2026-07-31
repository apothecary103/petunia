use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use gpui::Modifiers;
use serde::{Deserialize, Deserializer};

/// Everything a keypress can ask for. Adding a variant and a default binding is
/// the whole cost of a new shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    QuickSwitcher,
    Search,
    SearchThread,
    FocusComposer,
    ToggleSidebar,
    ToggleDetails,
    ScrollUp,
    ScrollDown,
    ScrollToTop,
    ScrollToBottom,
    NextUnread,
    NextConversation,
    PreviousConversation,
    MarkRead,
    ReplyToLast,
    EditLast,
    AttachFile,
    Cancel,
    Help,
    Settings,
}

/// A parsed chord. `cmd` is the platform key on macOS and control elsewhere, so
/// one config file works on both.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBind {
    pub modifiers: Modifiers,
    /// gpui's spelling of the key: a single character, or a name like
    /// `escape` or `pageup`.
    pub key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    bindings: HashMap<KeyBind, Action>,
}

impl Keys {
    /// Every binding as `action = "chord"`, for writing the config back out.
    /// Sorted, so a rewrite does not reshuffle the file.
    pub fn written(&self) -> Vec<(String, String)> {
        let mut written: Vec<_> = self
            .bindings
            .iter()
            .map(|(bind, action)| (bind.to_string(), name(*action).to_string()))
            .collect();
        written.sort_by(|a, b| a.1.cmp(&b.1));
        written
    }

    /// Every binding as a gpui keystroke string, for registering the keymap.
    pub fn bindings(&self) -> Vec<(String, Action)> {
        self.bindings
            .iter()
            .map(|(bind, action)| (bind.keystroke(), *action))
            .collect()
    }

    /// Which preset these came from, when they came from one unchanged. The
    /// settings window shows it, and shows nothing rather than lying when the
    /// bindings have been edited into something that is no preset at all.
    pub fn matches(&self) -> Option<Preset> {
        Preset::every()
            .into_iter()
            .find(|preset| Self::preset(*preset).bindings == self.bindings)
    }

    /// Every binding, for the help overlay, sorted so the list is stable.
    pub fn listing(&self) -> Vec<(String, Action)> {
        let mut listing: Vec<_> = self
            .bindings
            .iter()
            .map(|(bind, action)| (bind.to_string(), *action))
            .collect();
        listing.sort_by(|a, b| format!("{:?}", a.1).cmp(&format!("{:?}", b.1)));
        listing
    }
}

/// Which set of bindings to start from. Overrides in the config merge on top of
/// whichever is chosen, so a preset is a starting point rather than a cage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    /// What the platform's own applications do.
    #[default]
    Standard,
    /// Control chords, in the places emacs puts them.
    Emacs,
    /// Motions where vim puts them, on the understanding that a chat window has
    /// no modes to be in.
    Vim,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Emacs => "Emacs",
            Self::Vim => "Vim",
        }
    }

    pub fn every() -> [Self; 3] {
        [Self::Standard, Self::Emacs, Self::Vim]
    }

    fn bindings(self) -> &'static [(&'static str, Action)] {
        match self {
            Self::Standard => &STANDARD,
            Self::Emacs => &EMACS,
            Self::Vim => &VIM,
        }
    }
}

const STANDARD: [(&str, Action); 19] = [
    ("cmd+,", Action::Settings),
    ("cmd+f", Action::Search),
    ("cmd+shift+f", Action::SearchThread),
    ("cmd+k", Action::QuickSwitcher),
    ("cmd+l", Action::FocusComposer),
    ("cmd+b", Action::ToggleSidebar),
    ("cmd+i", Action::ToggleDetails),
    ("pageup", Action::ScrollUp),
    ("pagedown", Action::ScrollDown),
    ("cmd+home", Action::ScrollToTop),
    ("cmd+end", Action::ScrollToBottom),
    ("cmd+j", Action::NextUnread),
    ("cmd+shift+down", Action::NextConversation),
    ("cmd+shift+up", Action::PreviousConversation),
    ("cmd+shift+r", Action::MarkRead),
    ("cmd+r", Action::ReplyToLast),
    ("cmd+e", Action::EditLast),
    ("cmd+u", Action::AttachFile),
    ("cmd+/", Action::Help),
];

/// emacs by way of a chat window: `C-s` searches, `C-v`/`M-v` page, `C-x b`
/// switches buffers -- except that gpui binds single chords, so the two-key
/// sequences become one.
const EMACS: [(&str, Action); 19] = [
    ("ctrl+alt+,", Action::Settings),
    ("ctrl+s", Action::Search),
    ("alt+ctrl+s", Action::SearchThread),
    ("ctrl+x", Action::QuickSwitcher),
    ("ctrl+j", Action::FocusComposer),
    ("ctrl+b", Action::ToggleSidebar),
    ("alt+i", Action::ToggleDetails),
    ("alt+v", Action::ScrollUp),
    ("ctrl+v", Action::ScrollDown),
    ("alt+shift+,", Action::ScrollToTop),
    ("alt+shift+.", Action::ScrollToBottom),
    ("ctrl+n", Action::NextUnread),
    ("ctrl+alt+n", Action::NextConversation),
    ("ctrl+alt+p", Action::PreviousConversation),
    ("alt+r", Action::MarkRead),
    ("ctrl+r", Action::ReplyToLast),
    ("ctrl+alt+e", Action::EditLast),
    ("ctrl+alt+a", Action::AttachFile),
    ("ctrl+h", Action::Help),
];

/// vim's motions where vim puts them. A composer has no normal mode to leave,
/// so these are the chords that do not collide with typing: `ctrl+d`/`ctrl+u`
/// page, `g`/`G` become `cmd+g` and `cmd+shift+g`, and `/` searches.
const VIM: [(&str, Action); 19] = [
    ("cmd+,", Action::Settings),
    ("cmd+/", Action::Search),
    ("cmd+shift+/", Action::SearchThread),
    ("ctrl+p", Action::QuickSwitcher),
    ("cmd+i", Action::FocusComposer),
    ("cmd+shift+e", Action::ToggleSidebar),
    ("cmd+shift+i", Action::ToggleDetails),
    ("ctrl+u", Action::ScrollUp),
    ("ctrl+d", Action::ScrollDown),
    ("cmd+g", Action::ScrollToTop),
    ("cmd+shift+g", Action::ScrollToBottom),
    ("cmd+n", Action::NextUnread),
    ("ctrl+j", Action::NextConversation),
    ("ctrl+k", Action::PreviousConversation),
    ("cmd+shift+r", Action::MarkRead),
    ("cmd+r", Action::ReplyToLast),
    ("cmd+e", Action::EditLast),
    ("cmd+u", Action::AttachFile),
    ("cmd+shift+h", Action::Help),
];

impl Keys {
    pub fn preset(preset: Preset) -> Self {
        let mut bindings: HashMap<KeyBind, Action> = preset
            .bindings()
            .iter()
            .map(|(chord, action)| {
                (
                    chord
                        .parse()
                        .unwrap_or_else(|_| panic!("{preset:?} binding {chord}")),
                    *action,
                )
            })
            .collect();

        // Escape means the same thing everywhere. A preset that rebound it
        // would leave whichever overlay is up with no way out.
        bindings.insert("escape".parse().expect("escape parses"), Action::Cancel);
        Self { bindings }
    }
}

/// The spelling the config file uses, which is what `Action`'s own serde
/// derive produces -- written out here so that reading and writing cannot drift.
fn name(action: Action) -> &'static str {
    match action {
        Action::QuickSwitcher => "quick-switcher",
        Action::Search => "search",
        Action::SearchThread => "search-thread",
        Action::FocusComposer => "focus-composer",
        Action::ToggleSidebar => "toggle-sidebar",
        Action::ToggleDetails => "toggle-details",
        Action::ScrollUp => "scroll-up",
        Action::ScrollDown => "scroll-down",
        Action::ScrollToTop => "scroll-to-top",
        Action::ScrollToBottom => "scroll-to-bottom",
        Action::NextUnread => "next-unread",
        Action::NextConversation => "next-conversation",
        Action::PreviousConversation => "previous-conversation",
        Action::MarkRead => "mark-read",
        Action::ReplyToLast => "reply-to-last",
        Action::EditLast => "edit-last",
        Action::AttachFile => "attach-file",
        Action::Cancel => "cancel",
        Action::Help => "help",
        Action::Settings => "settings",
    }
}

impl Default for Keys {
    fn default() -> Self {
        Self::preset(Preset::default())
    }
}

/// Merged over the defaults, so binding one key does not silently drop the other
/// fifteen. A binding may be cleared by pointing it at `"none"`.
impl<'de> Deserialize<'de> for Keys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut overrides = HashMap::<String, String>::deserialize(deserializer)?;

        let preset = match overrides.remove("preset") {
            Some(name) => serde_json::from_value(serde_json::Value::String(name.clone()))
                .map_err(|_| serde::de::Error::custom(format!("unknown key preset: {name}")))?,
            None => Preset::default(),
        };
        let mut keys = Self::preset(preset);

        for (action, chord) in overrides {
            let action: Action = serde_json::from_value(serde_json::Value::String(action.clone()))
                .map_err(|_| serde::de::Error::custom(format!("unknown action: {action}")))?;

            // Rebinding replaces wherever that action used to live.
            keys.bindings.retain(|_, bound| *bound != action);
            if chord.eq_ignore_ascii_case("none") {
                continue;
            }
            let bind: KeyBind = chord
                .parse()
                .map_err(|error| serde::de::Error::custom(format!("{chord}: {error}")))?;
            keys.bindings.insert(bind, action);
        }
        Ok(keys)
    }
}

/// `cmd` means the platform key on macOS and control everywhere else, matching
/// what gpui calls `secondary`.
fn set_command(modifiers: &mut Modifiers) {
    if cfg!(target_os = "macos") {
        modifiers.platform = true;
    } else {
        modifiers.control = true;
    }
}

impl FromStr for KeyBind {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.trim().is_empty() {
            return Err("empty binding".to_owned());
        }

        let mut modifiers = Modifiers::default();
        let mut key: Option<String> = None;
        let set = |chord: String, key: &mut Option<String>| -> Result<(), String> {
            if key.is_some() {
                return Err(format!("more than one key in {raw:?}"));
            }
            *key = Some(chord);
            Ok(())
        };

        // The plus key is spelled by a trailing "+", as in "cmd++" or a bare
        // "+", which splitting leaves as two empty trailing parts.
        let mut parts: Vec<&str> = raw.split('+').collect();
        if parts.len() >= 2 && parts[parts.len() - 1].is_empty() && parts[parts.len() - 2].is_empty()
        {
            parts.truncate(parts.len() - 2);
            set("+".to_owned(), &mut key)?;
        }

        for part in parts {
            let part = part.trim();
            if part.is_empty() {
                return Err(format!("empty part in {raw:?}"));
            }
            match part.to_ascii_lowercase().as_str() {
                "cmd" | "command" | "super" | "meta" => set_command(&mut modifiers),
                "ctrl" | "control" => modifiers.control = true,
                "alt" | "option" | "opt" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                name => {
                    let chord = chord(name).ok_or_else(|| format!("unknown key: {name}"))?;
                    set(chord, &mut key)?;
                }
            }
        }

        key.map(|key| KeyBind { modifiers, key })
            .ok_or_else(|| format!("no key in {raw:?}"))
    }
}

/// Named keys petunia accepts. The spellings are gpui's, so a chord can be
/// handed straight to the keymap without translation.
const NAMED: &[&str] = &[
    "escape", "enter", "tab", "space", "backspace", "delete", "insert", "up", "down", "left",
    "right", "home", "end", "pageup", "pagedown", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8",
    "f9", "f10", "f11", "f12",
];

/// Spellings accepted but not produced.
const ALIASES: &[(&str, &str)] = &[
    ("esc", "escape"),
    ("return", "enter"),
    ("del", "delete"),
    ("pgup", "pageup"),
    ("pgdn", "pagedown"),
];

fn chord(name: &str) -> Option<String> {
    if NAMED.contains(&name) {
        return Some(name.to_owned());
    }
    if let Some((_, canonical)) = ALIASES.iter().find(|(alias, _)| *alias == name) {
        return Some((*canonical).to_owned());
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    chars
        .next()
        .is_none()
        .then(|| first.to_ascii_lowercase().to_string())
}

impl KeyBind {
    /// The gpui spelling, which uses `-` between parts.
    pub fn keystroke(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.platform {
            parts.push("cmd");
        }
        if self.modifiers.control {
            parts.push("ctrl");
        }
        if self.modifiers.alt {
            parts.push("alt");
        }
        if self.modifiers.shift {
            parts.push("shift");
        }
        parts.push(&self.key);
        parts.join("-")
    }
}

/// The spelling shown to the user and accepted in the config file, which uses
/// `+` between parts.
impl fmt::Display for KeyBind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers.platform {
            parts.push("cmd");
        }
        if self.modifiers.control {
            // Off macOS `cmd` *is* control, so it is named "cmd" to match what
            // the config file said rather than reading "ctrl" back.
            parts.push(if cfg!(target_os = "macos") { "ctrl" } else { "cmd" });
        }
        if self.modifiers.alt {
            parts.push("alt");
        }
        if self.modifiers.shift {
            parts.push("shift");
        }
        parts.push(&self.key);
        write!(formatter, "{}", parts.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Keystroke;

    fn bind(raw: &str) -> KeyBind {
        raw.parse().expect(raw)
    }

    fn command() -> Modifiers {
        let mut modifiers = Modifiers::default();
        set_command(&mut modifiers);
        modifiers
    }

    #[test]
    fn parses_a_bare_key() {
        assert_eq!(
            bind("k"),
            KeyBind {
                modifiers: Modifiers::default(),
                key: "k".to_owned(),
            }
        );
    }

    #[test]
    fn parses_modifiers_in_any_order() {
        assert_eq!(bind("cmd+shift+k"), bind("shift+cmd+k"));
    }

    #[test]
    fn command_maps_to_the_platform_modifier() {
        assert_eq!(bind("cmd+k").modifiers, command());
        assert_eq!(bind("command+k"), bind("super+k"));
    }

    #[test]
    fn keys_are_case_insensitive() {
        assert_eq!(bind("CMD+K"), bind("cmd+k"));
    }

    #[test]
    fn parses_named_keys_and_their_aliases() {
        assert_eq!(bind("esc"), bind("escape"));
        assert_eq!(bind("pgup"), bind("pageup"));
        assert_eq!(bind("enter").key, "enter");
    }

    #[test]
    fn rejects_nonsense() {
        for raw in ["", "cmd", "cmd+nope", "cmd+k+j", "shift"] {
            assert!(raw.parse::<KeyBind>().is_err(), "{raw:?} should not parse");
        }
    }

    #[test]
    fn the_plus_key_itself_can_be_bound() {
        assert_eq!(bind("cmd++").key, "+");
        assert_eq!(bind("cmd++").modifiers, command());
        assert_eq!(bind("+").key, "+");
        assert_eq!(bind("+").modifiers, Modifiers::default());
    }

    /// Bindings go into gpui's keymap as chord strings, so what the table holds
    /// is only ever seen through that spelling.
    fn bound(keys: &Keys, chord: &str) -> Option<Action> {
        keys.bindings()
            .into_iter()
            .find(|(keystroke, _)| keystroke == chord)
            .map(|(_, action)| action)
    }

    #[test]
    fn defaults_resolve_the_quick_switcher() {
        assert_eq!(
            bound(&Keys::default(), "cmd-k"),
            Some(Action::QuickSwitcher)
        );
    }

    #[test]
    fn a_bare_letter_is_not_a_shortcut() {
        assert_eq!(bound(&Keys::default(), "k"), None);
    }

    /// Unlike iced, gpui reports shift as a modifier rather than folding it into
    /// the character, so `cmd+shift+r` and `cmd+r` are genuinely different.
    #[test]
    fn shift_distinguishes_two_bindings() {
        let keys = Keys::default();

        assert_eq!(bound(&keys, "cmd-r"), Some(Action::ReplyToLast));
        assert_eq!(bound(&keys, "cmd-shift-r"), Some(Action::MarkRead));
    }

    #[test]
    fn escape_resolves_to_cancel() {
        assert_eq!(bound(&Keys::default(), "escape"), Some(Action::Cancel));
    }

    #[test]
    fn overrides_merge_over_the_defaults() {
        let keys: Keys = toml::from_str(r#"quick-switcher = "cmd+p""#).unwrap();

        assert_eq!(bound(&keys, "cmd-p"), Some(Action::QuickSwitcher));
        // The old binding is gone, and everything else survives.
        assert_eq!(bound(&keys, "cmd-k"), None);
        assert_eq!(bound(&keys, "escape"), Some(Action::Cancel));
    }

    #[test]
    fn a_binding_can_be_cleared() {
        let keys: Keys = toml::from_str(r#"quick-switcher = "none""#).unwrap();

        assert_eq!(bound(&keys, "cmd-k"), None);
    }

    #[test]
    fn an_unknown_action_is_reported() {
        assert!(toml::from_str::<Keys>(r#"teleport = "cmd+k""#).is_err());
    }

    #[test]
    fn an_unparseable_chord_is_reported() {
        assert!(toml::from_str::<Keys>(r#"quick-switcher = "cmd+nope""#).is_err());
    }

    #[test]
    fn every_default_action_is_bound_exactly_once() {
        let keys = Keys::default();
        let listing = keys.listing();

        let mut actions: Vec<_> = listing.iter().map(|(_, action)| *action).collect();
        let before = actions.len();
        actions.sort_by_key(|action| format!("{action:?}"));
        actions.dedup();

        assert_eq!(before, actions.len(), "an action is bound twice");
    }

    #[test]
    fn no_two_default_bindings_share_a_chord() {
        // The map key is the chord, so a collision would have silently dropped
        // one of them; this checks the count instead.
        assert_eq!(Keys::default().bindings.len(), Keys::default().listing().len());
    }

    #[test]
    fn display_round_trips_through_the_parser() {
        for raw in ["cmd+k", "cmd+shift+r", "escape", "pageup", "alt+left"] {
            let bind = bind(raw);
            assert_eq!(bind.to_string().parse::<KeyBind>().unwrap(), bind, "{raw}");
        }
    }

    /// Whatever we hand the keymap has to be something gpui can parse back, or
    /// the binding silently never fires.
    #[test]
    fn every_default_binding_is_a_valid_gpui_keystroke() {
        for (keystroke, action) in Keys::default().bindings() {
            let parsed = Keystroke::parse(&keystroke)
                .unwrap_or_else(|error| panic!("{action:?} bound to {keystroke:?}: {error:?}"));

            // Round-tripped through gpui's own parser: what it understands has
            // to be the same chord that went in, or the binding fires on a key
            // nobody meant.
            assert_eq!(parsed.unparse(), keystroke, "{action:?}");
        }
    }

    /// Every preset has to bind every action, or choosing one silently takes
    /// features away.
    #[test]
    fn every_preset_binds_every_action() {
        let standard: std::collections::HashSet<_> =
            Keys::preset(Preset::Standard).bindings.values().copied().collect();

        for preset in Preset::every() {
            let bound: std::collections::HashSet<_> =
                Keys::preset(preset).bindings.values().copied().collect();
            assert_eq!(bound, standard, "{preset:?}");
        }
    }

    /// A preset that bound one chord twice would silently lose an action.
    #[test]
    fn no_preset_binds_one_chord_to_two_actions() {
        for preset in Preset::every() {
            let keys = Keys::preset(preset);
            assert_eq!(
                keys.bindings.len(),
                keys.bindings.values().collect::<std::collections::HashSet<_>>().len(),
                "{preset:?}"
            );
        }
    }

    /// Whatever else a preset moves, escape stays where it is: it is the way out
    /// of every overlay, and there would be none if a preset took it.
    #[test]
    fn escape_always_cancels() {
        for preset in Preset::every() {
            assert_eq!(bound(&Keys::preset(preset), "escape"), Some(Action::Cancel));
        }
    }

    #[test]
    fn every_preset_binding_is_a_valid_gpui_keystroke() {
        for preset in Preset::every() {
            for (keystroke, action) in Keys::preset(preset).bindings() {
                Keystroke::parse(&keystroke).unwrap_or_else(|error| {
                    panic!("{preset:?} bound {action:?} to {keystroke:?}: {error:?}")
                });
            }
        }
    }

    #[test]
    fn a_preset_is_chosen_by_name() {
        let keys: Keys = toml::from_str(r#"preset = "emacs""#).unwrap();

        assert_eq!(bound(&keys, "ctrl-s"), Some(Action::Search));
        assert_eq!(bound(&keys, "cmd-f"), None);
    }

    /// A preset is a starting point, not a cage.
    #[test]
    fn overrides_merge_over_a_preset() {
        let keys: Keys = toml::from_str(
            r#"
                preset = "vim"
                help = "cmd+?"
            "#,
        )
        .unwrap();

        assert_eq!(bound(&keys, "ctrl-d"), Some(Action::ScrollDown));
        assert_eq!(bound(&keys, "cmd-?"), Some(Action::Help));
        assert_eq!(bound(&keys, "cmd-shift-h"), None);
    }

    #[test]
    fn an_unknown_preset_is_reported() {
        let error = toml::from_str::<Keys>(r#"preset = "dvorak""#).unwrap_err();

        assert!(error.to_string().contains("unknown key preset"), "{error}");
    }

    /// The settings window names the preset in force, and must not name one when
    /// the bindings have been edited away from it.
    #[test]
    fn an_edited_keymap_matches_no_preset() {
        for preset in Preset::every() {
            assert_eq!(Keys::preset(preset).matches(), Some(preset));
        }

        let edited: Keys = toml::from_str(r#"help = "cmd+?""#).unwrap();
        assert_eq!(edited.matches(), None);
    }
}
