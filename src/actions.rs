use gpui::{App, KeyBinding, Keystroke, actions};

use crate::config::keys::{Action, Keys};

actions!(
    petunia,
    [
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
        ThemePicker,
    ]
);

/// Installs the configured chords into gpui's keymap. Called at startup and
/// again on every hot reload, which replaces the previous bindings.
pub fn bind(keys: &Keys, cx: &mut App) {
    cx.clear_key_bindings();

    let bindings = keys
        .bindings()
        .into_iter()
        // `KeyBinding::new` panics on a chord gpui cannot parse, and these come
        // from a hand-edited file, so they are checked first.
        .filter(|(keystroke, action)| match Keystroke::parse(keystroke) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%keystroke, ?action, ?error, "unusable keybinding");
                false
            }
        })
        .map(|(keystroke, action)| binding(&keystroke, action))
        .collect::<Vec<_>>();

    cx.bind_keys(bindings);
}

fn binding(keystroke: &str, action: Action) -> KeyBinding {
    match action {
        Action::QuickSwitcher => KeyBinding::new(keystroke, QuickSwitcher, None),
        Action::Search => KeyBinding::new(keystroke, Search, None),
        Action::SearchThread => KeyBinding::new(keystroke, SearchThread, None),
        Action::FocusComposer => KeyBinding::new(keystroke, FocusComposer, None),
        Action::ToggleSidebar => KeyBinding::new(keystroke, ToggleSidebar, None),
        Action::ToggleDetails => KeyBinding::new(keystroke, ToggleDetails, None),
        Action::ScrollUp => KeyBinding::new(keystroke, ScrollUp, None),
        Action::ScrollDown => KeyBinding::new(keystroke, ScrollDown, None),
        Action::ScrollToTop => KeyBinding::new(keystroke, ScrollToTop, None),
        Action::ScrollToBottom => KeyBinding::new(keystroke, ScrollToBottom, None),
        Action::NextUnread => KeyBinding::new(keystroke, NextUnread, None),
        Action::NextConversation => KeyBinding::new(keystroke, NextConversation, None),
        Action::PreviousConversation => {
            KeyBinding::new(keystroke, PreviousConversation, None)
        }
        Action::MarkRead => KeyBinding::new(keystroke, MarkRead, None),
        Action::ReplyToLast => KeyBinding::new(keystroke, ReplyToLast, None),
        Action::EditLast => KeyBinding::new(keystroke, EditLast, None),
        Action::AttachFile => KeyBinding::new(keystroke, AttachFile, None),
        Action::Cancel => KeyBinding::new(keystroke, Cancel, None),
        Action::Help => KeyBinding::new(keystroke, Help, None),
        Action::Settings => KeyBinding::new(keystroke, Settings, None),
        Action::ThemePicker => KeyBinding::new(keystroke, ThemePicker, None),
    }
}
