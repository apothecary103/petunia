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
    NewChat,
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
    ThemePicker,
}

impl Action {
    /// Every variant, so "is everything reachable from the keyboard" is a
    /// question something can actually ask. Written out rather than derived,
    /// which is what makes forgetting to bind a new one a failing test.
    pub const EVERY: [Self; 22] = [
        Self::QuickSwitcher,
        Self::NewChat,
        Self::Search,
        Self::SearchThread,
        Self::FocusComposer,
        Self::ToggleSidebar,
        Self::ToggleDetails,
        Self::ScrollUp,
        Self::ScrollDown,
        Self::ScrollToTop,
        Self::ScrollToBottom,
        Self::NextUnread,
        Self::NextConversation,
        Self::PreviousConversation,
        Self::MarkRead,
        Self::ReplyToLast,
        Self::EditLast,
        Self::AttachFile,
        Self::Cancel,
        Self::Help,
        Self::Settings,
        Self::ThemePicker,
    ];
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
    /// Every action and the chords that reach it, for writing the config back
    /// out. Sorted, so a rewrite does not reshuffle the file.
    ///
    /// Grouped by *action* rather than one line per chord, because the file keys
    /// on the action: two lines naming the same one would be a duplicate key,
    /// which is not valid TOML and would have made the second chord for anything
    /// unwritable.
    pub fn written(&self) -> Vec<(String, Vec<String>)> {
        let mut grouped: HashMap<&'static str, Vec<String>> = HashMap::new();
        for (bind, action) in &self.bindings {
            grouped
                .entry(name(*action))
                .or_default()
                .push(bind.to_string());
        }

        let mut written: Vec<(String, Vec<String>)> = grouped
            .into_iter()
            .map(|(action, mut chords)| {
                chords.sort();
                (action.to_owned(), chords)
            })
            .collect();
        written.sort_by(|a, b| a.0.cmp(&b.0));
        written
    }

    /// Every binding as a gpui keystroke string, for registering the keymap.
    pub fn bindings(&self) -> Vec<(String, Action)> {
        self.bindings
            .iter()
            .map(|(bind, action)| (bind.keystroke(), *action))
            .collect()
    }

    /// Whether these are the shipped bindings, unedited. The settings window says
    /// so rather than offering a control that would undo an edit nobody made.
    pub fn are_default(&self) -> bool {
        Self::default().bindings == self.bindings
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

/// What every key does out of the box.
///
/// One set, not a choice of three. The emacs and vim presets were a promise this
/// cannot keep: a preset is only a preset if it goes all the way down, and a chat
/// window has no modal editing, no kill ring and no buffer list to bind — so what
/// they actually were was the same twenty verbs under unfamiliar chords, three
/// tables to keep in step, and a settings row asking a question with no good
/// answer. What people wanted from them was `ctrl+p` and `ctrl+n`, which are here
/// for everybody instead.
///
/// Several chords may name the same action, and several do: the platform chord for
/// people who know it, and the control chord and the arrow beside it for people
/// whose hands are already there. `written` and `listing` both cope, and a config
/// override replaces every chord for the action it names.
const DEFAULTS: &[(&str, Action)] = &[
    // Windows and panels.
    ("cmd+,", Action::Settings),
    ("cmd+/", Action::Help),
    ("cmd+shift+t", Action::ThemePicker),
    ("cmd+b", Action::ToggleSidebar),
    ("cmd+i", Action::ToggleDetails),
    // Getting somewhere.
    ("cmd+k", Action::QuickSwitcher),
    ("ctrl+space", Action::QuickSwitcher),
    ("cmd+n", Action::NewChat),
    ("cmd+f", Action::Search),
    ("cmd+shift+f", Action::SearchThread),
    ("cmd+j", Action::NextUnread),
    // Moving between conversations. `ctrl+n` and `ctrl+p` are the two chords
    // people reach for wherever a list has a cursor, and the arrows are what
    // everybody else tries first.
    ("ctrl+n", Action::NextConversation),
    ("ctrl+p", Action::PreviousConversation),
    ("alt+down", Action::NextConversation),
    ("alt+up", Action::PreviousConversation),
    ("cmd+shift+down", Action::NextConversation),
    ("cmd+shift+up", Action::PreviousConversation),
    // Moving through one. Bare arrows and bare page keys reach the list only when
    // nothing else has the focus, since a text field consumes them first -- which
    // is exactly the arrangement wanted: they scroll the conversation when you are
    // reading it and move the caret when you are writing.
    ("up", Action::ScrollUp),
    ("down", Action::ScrollDown),
    ("pageup", Action::ScrollUp),
    ("pagedown", Action::ScrollDown),
    ("ctrl+alt+p", Action::ScrollUp),
    ("ctrl+alt+n", Action::ScrollDown),
    ("home", Action::ScrollToTop),
    ("end", Action::ScrollToBottom),
    ("cmd+home", Action::ScrollToTop),
    ("cmd+end", Action::ScrollToBottom),
    // Doing something about it.
    ("cmd+l", Action::FocusComposer),
    ("cmd+r", Action::ReplyToLast),
    ("cmd+e", Action::EditLast),
    ("cmd+u", Action::AttachFile),
    ("cmd+shift+r", Action::MarkRead),
];

impl Keys {
    /// The shipped bindings.
    pub fn shipped() -> Self {
        let mut bindings: HashMap<KeyBind, Action> = DEFAULTS
            .iter()
            .map(|(chord, action)| {
                (
                    chord
                        .parse()
                        .unwrap_or_else(|_| panic!("default binding {chord}")),
                    *action,
                )
            })
            .collect();

        // Escape means the same thing everywhere, and it is not in the table
        // above because it is not up for discussion: rebinding it would leave
        // whichever overlay is up with no way out.
        bindings.insert("escape".parse().expect("escape parses"), Action::Cancel);
        Self { bindings }
    }
}

/// The spelling the config file uses, which is what `Action`'s own serde
/// derive produces -- written out here so that reading and writing cannot drift.
fn name(action: Action) -> &'static str {
    match action {
        Action::QuickSwitcher => "quick-switcher",
        Action::NewChat => "new-chat",
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
        Action::ThemePicker => "theme-picker",
    }
}

impl Default for Keys {
    fn default() -> Self {
        Self::shipped()
    }
}

/// One chord, or several. An action reached two ways is the normal case here, so
/// the file says so in the shape a person would write it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Chords {
    One(String),
    Many(Vec<String>),
}

impl Chords {
    fn all(self) -> Vec<String> {
        match self {
            Self::One(chord) => vec![chord],
            Self::Many(chords) => chords,
        }
    }
}

/// Merged over the defaults, so binding one key does not silently drop the other
/// fifteen. A binding may be cleared by pointing it at `"none"`.
impl<'de> Deserialize<'de> for Keys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let overrides = HashMap::<String, Chords>::deserialize(deserializer)?;
        let mut keys = Self::shipped();

        for (action, chords) in overrides {
            let action: Action = serde_json::from_value(serde_json::Value::String(action.clone()))
                .map_err(|_| serde::de::Error::custom(format!("unknown action: {action}")))?;

            // Rebinding replaces wherever that action used to live -- all of it,
            // since an action may have arrived here with several chords and
            // naming one of them is not a request to keep the rest.
            keys.bindings.retain(|_, bound| *bound != action);

            for chord in chords.all() {
                if chord.eq_ignore_ascii_case("none") {
                    continue;
                }
                let bind: KeyBind = chord
                    .parse()
                    .map_err(|error| serde::de::Error::custom(format!("{chord}: {error}")))?;
                keys.bindings.insert(bind, action);
            }
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


    /// Every action has to be reachable, or a verb exists with no way to ask for
    /// it. Escape is in the set too: it is inserted rather than tabled.
    #[test]
    fn every_action_is_bound() {
        let bound: std::collections::HashSet<_> =
            Keys::shipped().bindings.values().copied().collect();

        for action in Action::EVERY {
            assert!(bound.contains(&action), "{action:?} is bound to nothing");
        }
    }

    /// One chord may not mean two things. A `HashMap` keyed by the chord makes
    /// that impossible to represent, which is the point -- so what this really
    /// checks is that the table did not *silently lose* an entry to a collision.
    #[test]
    fn no_chord_is_written_twice() {
        assert_eq!(
            DEFAULTS.len() + 1,
            Keys::shipped().bindings.len(),
            "two default chords collide, so one of them was dropped"
        );
    }

    /// Several chords for one action is deliberate, and the two chords people
    /// actually asked for are the ones worth pinning.
    #[test]
    fn the_control_chords_move_between_conversations() {
        let keys = Keys::shipped();

        assert_eq!(bound(&keys, "ctrl-n"), Some(Action::NextConversation));
        assert_eq!(bound(&keys, "ctrl-p"), Some(Action::PreviousConversation));
        assert_eq!(bound(&keys, "alt-down"), Some(Action::NextConversation));
        assert_eq!(bound(&keys, "alt-up"), Some(Action::PreviousConversation));
    }

    /// The bare arrows and page keys reach the list only when no field has the
    /// focus, which is what makes them safe to bind at all.
    #[test]
    fn the_arrows_scroll_the_conversation() {
        let keys = Keys::shipped();

        assert_eq!(bound(&keys, "up"), Some(Action::ScrollUp));
        assert_eq!(bound(&keys, "down"), Some(Action::ScrollDown));
        assert_eq!(bound(&keys, "home"), Some(Action::ScrollToTop));
        assert_eq!(bound(&keys, "end"), Some(Action::ScrollToBottom));
    }

    /// Escape is the way out of every overlay, and rebinding it would leave
    /// none.
    #[test]
    fn escape_always_cancels() {
        assert_eq!(bound(&Keys::shipped(), "escape"), Some(Action::Cancel));
    }

    /// An override replaces *every* chord the action had, or rebinding one of a
    /// pair would leave the other answering the old way.
    #[test]
    fn an_override_replaces_every_chord_for_its_action() {
        let keys: Keys = toml::from_str(r#"next-conversation = "cmd+]""#).unwrap();

        assert_eq!(bound(&keys, "cmd-]"), Some(Action::NextConversation));
        assert_eq!(bound(&keys, "ctrl-n"), None);
        assert_eq!(bound(&keys, "alt-down"), None);
        // And nothing else moved.
        assert_eq!(bound(&keys, "ctrl-p"), Some(Action::PreviousConversation));
    }

    /// The settings window says whether the keymap is the shipped one, and must
    /// not say so once it has been edited.
    #[test]
    fn an_edited_keymap_is_not_the_default_one() {
        assert!(Keys::shipped().are_default());

        let edited: Keys = toml::from_str(r#"help = "cmd+?""#).unwrap();
        assert!(!edited.are_default());
    }
}
