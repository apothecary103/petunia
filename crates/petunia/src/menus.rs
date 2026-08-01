//! The menu bar macOS puts at the top of the screen.
//!
//! Every item is one of the actions the keymap already dispatches, so a menu and
//! a keystroke are the same code path and the shortcut beside an item is read out
//! of the bindings actually in force rather than typed in beside it. Nothing here
//! is a control of its own: an item with no action behind it would be a menu that
//! lies.

use gpui::{App, Menu, MenuItem, OsAction};
use gpui_component::input;

use crate::actions::*;

pub fn install(cx: &mut App) {
    // The application menu takes its name from the bundle, not from here, but
    // the rest is ours.
    cx.set_menus([
        Menu::new("Petunia").items([
            MenuItem::action("Settings…", Settings),
            MenuItem::action("Theme…", ThemePicker),
            MenuItem::separator(),
            MenuItem::action("Hide Petunia", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::separator(),
            MenuItem::action("Quit Petunia", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", input::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Find…", Search),
            MenuItem::action("Find in Conversation…", SearchThread),
        ]),
        Menu::new("Message").items([
            MenuItem::action("New Conversation…", NewChat),
            MenuItem::separator(),
            MenuItem::action("Reply to Last", ReplyToLast),
            MenuItem::action("Edit Last", EditLast),
            MenuItem::separator(),
            MenuItem::action("Attach File…", AttachFile),
            MenuItem::separator(),
            MenuItem::action("Mark as Read", MarkRead),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Conversation List", ToggleSidebar),
            MenuItem::action("Toggle Details", ToggleDetails),
            MenuItem::separator(),
            MenuItem::action("Focus Composer", FocusComposer),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Go to Conversation…", QuickSwitcher),
            MenuItem::separator(),
            MenuItem::action("Next Conversation", NextConversation),
            MenuItem::action("Previous Conversation", PreviousConversation),
            MenuItem::action("Next Unread", NextUnread),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
        Menu::new("Help").items([MenuItem::action("Keyboard Shortcuts", Help)]),
    ]);
}
