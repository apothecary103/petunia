//! What right-clicking a conversation offers.

use std::rc::Rc;

use gpui::Window;
use gpui_component::IconName;

use super::Item;
use petunia_data::index::Flags;

/// What to do with the flags an entry produces. A callback rather than a store
/// handle, so building a menu is a pure function of what is true about the
/// conversation and can be tested as one.
pub type Apply = Rc<dyn Fn(Flags, &mut Window, &mut gpui::App)>;

/// How long "mute" mutes for. Signal's own choices, and the reason `muted_until`
/// is an instant: eight hours has to still be eight hours after a restart.
const DURATIONS: [(&str, u64); 4] = [
    ("For an hour", 3_600),
    ("For eight hours", 8 * 3_600),
    ("For a week", 7 * 24 * 3_600),
    ("Until I turn it back on", 0),
];

/// What a "new folder" entry does: ask for a name, and put this conversation in
/// it. Separate from `Apply` because naming one needs a text field, which a menu
/// cannot hold — it closes on the click that would focus it.
pub type Create = Rc<dyn Fn(&mut Window, &mut gpui::App)>;

/// What "delete" does, which is ask first. Separate from `Apply` for the same
/// reason `Create` is: it needs a dialog, not a flag.
pub type Delete = Rc<dyn Fn(&mut Window, &mut gpui::App)>;

/// The menu for a conversation in the list.
pub fn items(
    flags: &Flags,
    folders: &[String],
    now: u64,
    apply: Apply,
    create: Create,
    delete: Delete,
) -> Vec<Item> {
    let mut items = vec![
        toggle(
            &apply,
            flags,
            "Pin",
            IconName::Star,
            flags.pinned,
            |flags| Flags {
                pinned: !flags.pinned,
                // Pinning something you had put away is a decision to see it
                // again, and leaving it archived would mean it stayed hidden.
                archived: false,
                ..flags
            },
        ),
        toggle(
            &apply,
            flags,
            "Archive",
            IconName::Inbox,
            flags.archived,
            |flags| Flags {
                archived: !flags.archived,
                pinned: false,
                ..flags
            },
        ),
        Item::Separator,
    ];

    if flags.muted(now) {
        items.push(
            set(&apply, flags, "Unmute", |flags| Flags {
                muted_until: None,
                ..flags
            })
            .icon(IconName::Bell),
        );
    } else {
        items.push(Item::Label("Mute".into()));
        for (label, seconds) in DURATIONS {
            let until = match seconds {
                // Far enough out to mean "until I say otherwise" without
                // needing a second way of saying it.
                0 => u64::MAX,
                seconds => now + seconds * 1_000,
            };
            items.push(set(&apply, flags, label, move |flags| Flags {
                muted_until: Some(until),
                ..flags
            }));
        }
    }

    items.push(Item::Separator);
    items.push(Item::Label("Folder".into()));
    items.push(
        Item::new("New folder…", move |window, cx| create(window, cx))
            .icon(IconName::FolderOpen),
    );
    items.push(
        set(&apply, flags, "None", |flags| Flags {
            folder: None,
            ..flags
        })
        .checked(flags.folder.is_none()),
    );
    for folder in folders {
        let name = folder.clone();
        let chosen = flags.folder.as_deref() == Some(folder.as_str());
        items.push(
            set(&apply, flags, folder.clone(), move |flags| Flags {
                folder: Some(name.clone()),
                ..flags
            })
            .checked(chosen),
        );
    }

    // Last, and behind a separator: the one entry here that throws something
    // away, kept as far as possible from the ones that merely file it. The
    // ellipsis is the promise that it asks first.
    items.push(Item::Separator);
    items.push(
        Item::new("Delete conversation…", move |window, cx| delete(window, cx))
            .icon(IconName::Delete)
            .danger(),
    );

    items
}

/// A menu entry that applies whatever the given change makes of the flags.
fn set(
    apply: &Apply,
    flags: &Flags,
    label: impl Into<gpui::SharedString>,
    change: impl Fn(Flags) -> Flags + 'static,
) -> Item {
    let apply = apply.clone();
    let flags = flags.clone();

    Item::new(label, move |window: &mut Window, cx: &mut gpui::App| {
        apply(change(flags.clone()), window, cx)
    })
}

fn toggle(
    apply: &Apply,
    flags: &Flags,
    label: &'static str,
    icon: IconName,
    on: bool,
    change: impl Fn(Flags) -> Flags + 'static,
) -> Item {
    set(apply, flags, label, change).icon(icon).checked(on)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Entry { label, .. } => Some(label.to_string()),
                _ => None,
            })
            .collect()
    }

    /// What is offered, which is all these check -- not what it does.
    fn menu(flags: Flags, folders: &[String], now: u64) -> Vec<Item> {
        items(
            &flags,
            folders,
            now,
            Rc::new(|_, _, _| {}),
            Rc::new(|_, _| {}),
            Rc::new(|_, _| {}),
        )
    }

    /// Without this there is no way to make the first folder, and the whole
    /// feature is unreachable.
    #[test]
    fn a_folder_can_always_be_made() {
        assert!(labels(&menu(Flags::default(), &[], 0)).contains(&"New folder…".to_string()));
    }

    #[test]
    fn an_unmuted_conversation_offers_durations() {
        let offered = labels(&menu(Flags::default(), &[], 1_000));

        assert!(offered.contains(&"For an hour".to_string()));
        assert!(!offered.contains(&"Unmute".to_string()));
    }

    #[test]
    fn a_muted_conversation_offers_only_unmute() {
        let flags = Flags {
            muted_until: Some(9_000),
            ..Flags::default()
        };

        let offered = labels(&menu(flags, &[], 1_000));

        assert!(offered.contains(&"Unmute".to_string()));
        assert!(!offered.contains(&"For an hour".to_string()));
    }

    /// A mute that has run out is not a mute, and offering "unmute" for one
    /// would be a control that does nothing.
    #[test]
    fn an_expired_mute_offers_durations_again() {
        let flags = Flags {
            muted_until: Some(500),
            ..Flags::default()
        };

        let offered = labels(&menu(flags, &[], 1_000));

        assert!(offered.contains(&"For an hour".to_string()));
    }

    /// The ellipsis is a promise that it asks first, and the danger colour is how
    /// the one destructive entry is told apart from the ones that merely file
    /// something.
    #[test]
    fn deleting_is_offered_last_asks_first_and_is_marked_destructive() {
        let items = menu(Flags::default(), &[], 0);

        let last = items.last().expect("an entry");
        assert!(matches!(
            last,
            Item::Entry { label, danger: true, .. } if label.ends_with('…')
        ));
        assert!(
            labels(&items).contains(&"Delete conversation…".to_string()),
            "{:?}",
            labels(&items)
        );
    }

    /// Nothing about a conversation should hide the way to delete it -- least of
    /// all archiving, which is where the ones you have finished with end up.
    #[test]
    fn deleting_is_offered_whatever_state_the_conversation_is_in() {
        let states = [
            Flags::default(),
            Flags { pinned: true, ..Default::default() },
            Flags { archived: true, ..Default::default() },
            Flags { muted_until: Some(9_000), ..Default::default() },
            Flags { folder: Some("Work".into()), ..Default::default() },
        ];

        for flags in states {
            assert!(
                labels(&menu(flags.clone(), &["Work".to_string()], 1_000))
                    .contains(&"Delete conversation…".to_string()),
                "{flags:?}"
            );
        }
    }

    #[test]
    fn every_folder_is_offered_with_a_way_out() {
        let folders = vec!["Work".to_string(), "Family".to_string()];

        let offered = labels(&menu(Flags::default(), &folders, 0));

        assert!(offered.contains(&"Work".to_string()));
        assert!(offered.contains(&"Family".to_string()));
        assert!(offered.contains(&"None".to_string()));
    }
}
