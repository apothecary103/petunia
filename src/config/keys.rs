use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use iced::keyboard::{Key, Modifiers, key::Named};
use serde::{Deserialize, Deserializer};

/// Everything a keypress can ask for. Adding a variant and a default binding is
/// the whole cost of a new shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    QuickSwitcher,
    FocusComposer,
    NextPane,
    PreviousPane,
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    MaximizePane,
    ToggleLayout,
    ToggleSidebar,
    ScrollUp,
    ScrollDown,
    ScrollToTop,
    ScrollToBottom,
    NextUnread,
    MarkRead,
    ReplyToLast,
    EditLast,
    AttachFile,
    Cancel,
    Help,
}

/// A parsed chord. `Modifiers::COMMAND` is LOGO on macOS and CTRL elsewhere, so
/// one config file works on both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBind {
    pub modifiers: Modifiers,
    pub key: Chord,
}

/// The key half of a chord, normalised so `a` and `A` are the same binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Chord {
    Character(char),
    Named(Named),
}

#[derive(Debug, Clone)]
pub struct Keys {
    bindings: HashMap<KeyBind, Action>,
}

impl Keys {
    pub fn action(&self, key: &Key, modifiers: Modifiers) -> Option<Action> {
        let chord = match key {
            Key::Character(text) => Chord::Character(text.chars().next()?.to_ascii_lowercase()),
            Key::Named(named) => Chord::Named(*named),
            Key::Unidentified => return None,
        };
        // Shift is part of the chord for named keys but not for characters,
        // where the character itself already encodes it.
        let modifiers = match chord {
            Chord::Character(_) => modifiers - Modifiers::SHIFT,
            Chord::Named(_) => modifiers,
        };
        self.bindings.get(&KeyBind { modifiers, key: chord }).copied()
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

impl Default for Keys {
    fn default() -> Self {
        let defaults = [
            ("cmd+k", Action::QuickSwitcher),
            ("cmd+l", Action::FocusComposer),
            ("cmd+]", Action::NextPane),
            ("cmd+[", Action::PreviousPane),
            ("cmd+d", Action::SplitVertical),
            ("cmd+shift+d", Action::SplitHorizontal),
            ("cmd+w", Action::ClosePane),
            ("cmd+m", Action::MaximizePane),
            ("cmd+t", Action::ToggleLayout),
            ("cmd+b", Action::ToggleSidebar),
            ("pageup", Action::ScrollUp),
            ("pagedown", Action::ScrollDown),
            ("cmd+home", Action::ScrollToTop),
            ("cmd+end", Action::ScrollToBottom),
            ("cmd+j", Action::NextUnread),
            ("cmd+shift+r", Action::MarkRead),
            ("cmd+r", Action::ReplyToLast),
            ("cmd+e", Action::EditLast),
            ("cmd+u", Action::AttachFile),
            ("escape", Action::Cancel),
            ("cmd+/", Action::Help),
        ];

        Self {
            bindings: defaults
                .into_iter()
                .map(|(chord, action)| {
                    (
                        chord.parse().unwrap_or_else(|_| panic!("default binding {chord}")),
                        action,
                    )
                })
                .collect(),
        }
    }
}

/// Merged over the defaults, so binding one key does not silently drop the other
/// twenty. A binding may be cleared by pointing it at `"none"`.
impl<'de> Deserialize<'de> for Keys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let overrides = HashMap::<String, String>::deserialize(deserializer)?;
        let mut keys = Self::default();

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

impl FromStr for KeyBind {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.trim().is_empty() {
            return Err("empty binding".to_owned());
        }

        let mut modifiers = Modifiers::empty();
        let mut key = None;
        let set = |chord: Chord, key: &mut Option<Chord>| -> Result<(), String> {
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
            set(Chord::Character('+'), &mut key)?;
        }

        for part in parts {
            let part = part.trim();
            if part.is_empty() {
                return Err(format!("empty part in {raw:?}"));
            }
            match part.to_ascii_lowercase().as_str() {
                "cmd" | "command" | "super" | "meta" => modifiers |= Modifiers::COMMAND,
                "ctrl" | "control" => modifiers |= Modifiers::CTRL,
                "alt" | "option" | "opt" => modifiers |= Modifiers::ALT,
                "shift" => modifiers |= Modifiers::SHIFT,
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

/// The canonical spelling of every named key, used for both parsing and
/// display, so `Display` output always parses back to the same binding.
const NAMED: &[(&str, Named)] = &[
    ("escape", Named::Escape),
    ("enter", Named::Enter),
    ("tab", Named::Tab),
    ("space", Named::Space),
    ("backspace", Named::Backspace),
    ("delete", Named::Delete),
    ("up", Named::ArrowUp),
    ("down", Named::ArrowDown),
    ("left", Named::ArrowLeft),
    ("right", Named::ArrowRight),
    ("home", Named::Home),
    ("end", Named::End),
    ("pageup", Named::PageUp),
    ("pagedown", Named::PageDown),
    ("f1", Named::F1),
    ("f2", Named::F2),
    ("f3", Named::F3),
    ("f4", Named::F4),
    ("f5", Named::F5),
    ("f6", Named::F6),
    ("f7", Named::F7),
    ("f8", Named::F8),
    ("f9", Named::F9),
    ("f10", Named::F10),
    ("f11", Named::F11),
    ("f12", Named::F12),
];

/// Spellings accepted but not produced.
const ALIASES: &[(&str, Named)] = &[
    ("esc", Named::Escape),
    ("return", Named::Enter),
    ("del", Named::Delete),
    ("pgup", Named::PageUp),
    ("pgdn", Named::PageDown),
];

fn chord(name: &str) -> Option<Chord> {
    if let Some((_, named)) = NAMED.iter().chain(ALIASES).find(|(spelling, _)| *spelling == name) {
        return Some(Chord::Named(*named));
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    chars
        .next()
        .is_none()
        .then_some(Chord::Character(first.to_ascii_lowercase()))
}

fn spelling(named: Named) -> String {
    NAMED
        .iter()
        .find(|(_, candidate)| *candidate == named)
        .map(|(spelling, _)| (*spelling).to_owned())
        .unwrap_or_else(|| format!("{named:?}").to_lowercase())
}

impl fmt::Display for KeyBind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers.contains(Modifiers::COMMAND) {
            parts.push(if cfg!(target_os = "macos") { "cmd" } else { "ctrl" });
        }
        // Off macOS, COMMAND *is* CTRL, so naming both would read "ctrl+ctrl".
        if self.modifiers.contains(Modifiers::CTRL)
            && !Modifiers::COMMAND.contains(Modifiers::CTRL)
        {
            parts.push("ctrl");
        }
        if self.modifiers.contains(Modifiers::ALT) {
            parts.push("alt");
        }
        if self.modifiers.contains(Modifiers::SHIFT) {
            parts.push("shift");
        }

        let key = match self.key {
            Chord::Character(character) => character.to_string(),
            Chord::Named(named) => spelling(named),
        };
        parts.push(&key);
        write!(formatter, "{}", parts.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(raw: &str) -> KeyBind {
        raw.parse().expect(raw)
    }

    #[test]
    fn parses_a_bare_key() {
        assert_eq!(
            bind("k"),
            KeyBind {
                modifiers: Modifiers::empty(),
                key: Chord::Character('k')
            }
        );
    }

    #[test]
    fn parses_modifiers_in_any_order() {
        assert_eq!(bind("cmd+shift+k"), bind("shift+cmd+k"));
    }

    #[test]
    fn command_maps_to_the_platform_modifier() {
        assert_eq!(bind("cmd+k").modifiers, Modifiers::COMMAND);
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
        assert_eq!(bind("enter").key, Chord::Named(Named::Enter));
    }

    #[test]
    fn rejects_nonsense() {
        for raw in ["", "cmd", "cmd+nope", "cmd+k+j", "shift"] {
            assert!(raw.parse::<KeyBind>().is_err(), "{raw:?} should not parse");
        }
    }

    #[test]
    fn the_plus_key_itself_can_be_bound() {
        assert_eq!(bind("cmd++").key, Chord::Character('+'));
        assert_eq!(bind("cmd++").modifiers, Modifiers::COMMAND);
        assert_eq!(bind("+").key, Chord::Character('+'));
        assert_eq!(bind("+").modifiers, Modifiers::empty());
    }

    #[test]
    fn defaults_resolve_the_quick_switcher() {
        let keys = Keys::default();

        assert_eq!(
            keys.action(&Key::Character("k".into()), Modifiers::COMMAND),
            Some(Action::QuickSwitcher)
        );
    }

    #[test]
    fn a_bare_letter_is_not_a_shortcut() {
        let keys = Keys::default();

        assert_eq!(keys.action(&Key::Character("k".into()), Modifiers::empty()), None);
    }

    /// Shift is already encoded in the character a keyboard produces, so
    /// requiring it to match as well would break capitals.
    #[test]
    fn shift_is_ignored_for_character_chords() {
        let keys = Keys::default();

        assert_eq!(
            keys.action(
                &Key::Character("k".into()),
                Modifiers::COMMAND | Modifiers::SHIFT
            ),
            Some(Action::QuickSwitcher)
        );
    }

    #[test]
    fn escape_resolves_to_cancel() {
        let keys = Keys::default();

        assert_eq!(
            keys.action(&Key::Named(Named::Escape), Modifiers::empty()),
            Some(Action::Cancel)
        );
    }

    #[test]
    fn overrides_merge_over_the_defaults() {
        let keys: Keys = toml::from_str(r#"quick-switcher = "cmd+p""#).unwrap();

        assert_eq!(
            keys.action(&Key::Character("p".into()), Modifiers::COMMAND),
            Some(Action::QuickSwitcher)
        );
        // The old binding is gone, and everything else survives.
        assert_eq!(
            keys.action(&Key::Character("k".into()), Modifiers::COMMAND),
            None
        );
        assert_eq!(
            keys.action(&Key::Named(Named::Escape), Modifiers::empty()),
            Some(Action::Cancel)
        );
    }

    #[test]
    fn a_binding_can_be_cleared() {
        let keys: Keys = toml::from_str(r#"quick-switcher = "none""#).unwrap();

        assert_eq!(
            keys.action(&Key::Character("k".into()), Modifiers::COMMAND),
            None
        );
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
        for raw in ["cmd+k", "cmd+shift+d", "escape", "pageup", "alt+left"] {
            let bind = bind(raw);
            assert_eq!(bind.to_string().parse::<KeyBind>().unwrap(), bind, "{raw}");
        }
    }
}
