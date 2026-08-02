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

/// Who a one-to-one conversation is with, and what is already true about them.
/// `None` for a group, which has nobody to nickname and nobody to block.
#[derive(Debug, Clone, Copy)]
pub struct Person {
    pub who: uuid::Uuid,
    pub blocked: bool,
}

/// What the two entries about the person do. Both need more than a flag -- a
/// nickname needs a field and blocking asks first -- so both are callbacks, and
/// blocking is told which way it is going rather than reading it back.
pub type Name = Rc<dyn Fn(uuid::Uuid, &mut Window, &mut gpui::App)>;
pub type Block = Rc<dyn Fn(uuid::Uuid, bool, &mut Window, &mut gpui::App)>;

/// What every entry in the menu does. One struct rather than five arguments:
/// they are all the same kind of thing, and a caller passing them positionally
/// is a caller who can swap two of them silently.
pub struct Acts {
    pub apply: Apply,
    pub create: Create,
    pub delete: Delete,
    pub name: Name,
    pub block: Block,
    pub expire: Expire,
}

/// Setting the disappearing-message timer, in seconds. Not a flag: it is not
/// this device's opinion about the conversation but the conversation's own
/// setting, and everybody in it is told.
pub type Expire = Rc<dyn Fn(u32, &mut Window, &mut gpui::App)>;

/// How long a message lives. Signal's own ladder, and the reason it is a ladder
/// rather than a field: every one of these is a decision somebody can make in a
/// second, and a box that takes a number of seconds is a box nobody fills in.
pub const EXPIRIES: [(&str, u32); 8] = [
    ("Off", 0),
    ("30 seconds", 30),
    ("5 minutes", 5 * 60),
    ("1 hour", 3_600),
    ("8 hours", 8 * 3_600),
    ("1 day", 24 * 3_600),
    ("1 week", 7 * 24 * 3_600),
    ("4 weeks", 4 * 7 * 24 * 3_600),
];

/// The menu for a conversation in the list.
pub fn items(
    flags: &Flags,
    folders: &[String],
    now: u64,
    person: Option<Person>,
    timer: Option<std::time::Duration>,
    acts: Acts,
) -> Vec<Item> {
    let Acts {
        apply,
        create,
        delete,
        name,
        block,
        expire,
    } = acts;
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

    // Disappearing messages, as the ladder rather than as a switch: "on" is not
    // a setting anybody can act on without also being asked how long.
    items.push(Item::Separator);
    items.push(Item::Label("Disappearing messages".into()));
    let set_to = timer.map(|timer| timer.as_secs() as u32).unwrap_or(0);
    for (label, seconds) in EXPIRIES {
        let expire = expire.clone();
        items.push(
            Item::new(label, move |window: &mut Window, cx: &mut gpui::App| {
                expire(seconds, window, cx)
            })
            .checked(set_to == seconds),
        );
    }

    // What is true about the person rather than about the conversation. Here
    // rather than only in the details panel: the list is where you already are
    // when you decide you have had enough of somebody, and a panel you have to
    // open first to reach a verb is a verb behind a door.
    if let Some(person) = person {
        items.push(Item::Separator);
        items.push(
            Item::new("Set nickname…", move |window: &mut Window, cx: &mut gpui::App| {
                name(person.who, window, cx)
            })
            .icon(IconName::Replace),
        );
        // A toggle written as one entry that reads the way it acts. Both at
        // once would be a menu where one of them does nothing.
        items.push(match person.blocked {
            true => Item::new("Unblock", {
                let block = block.clone();
                move |window: &mut Window, cx: &mut gpui::App| block(person.who, false, window, cx)
            })
            .icon(IconName::CircleCheck),
            false => Item::new("Block…", move |window: &mut Window, cx: &mut gpui::App| {
                block(person.who, true, window, cx)
            })
            .icon(IconName::CircleX)
            .danger(),
        });
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
        with(flags, folders, now, None)
    }

    fn with(flags: Flags, folders: &[String], now: u64, person: Option<Person>) -> Vec<Item> {
        items(
            &flags,
            folders,
            now,
            person,
            None,
            Acts {
                apply: Rc::new(|_, _, _| {}),
                create: Rc::new(|_, _| {}),
                delete: Rc::new(|_, _| {}),
                name: Rc::new(|_, _, _| {}),
                block: Rc::new(|_, _, _, _| {}),
                expire: Rc::new(|_, _, _| {}),
            },
        )
    }

    fn person(blocked: bool) -> Option<Person> {
        Some(Person {
            who: uuid::Uuid::nil(),
            blocked,
        })
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

    /// The list is where you already are when you decide what to do about
    /// somebody, so both verbs are reachable without opening a panel first.
    #[test]
    fn a_conversation_with_one_person_offers_a_nickname_and_a_block() {
        let offered = labels(&with(Flags::default(), &[], 0, person(false)));

        assert!(offered.contains(&"Set nickname…".to_string()));
        assert!(offered.contains(&"Block…".to_string()));
    }

    /// Blocking is a toggle, and a menu offering both ways of it would be a menu
    /// where one of them does nothing.
    #[test]
    fn somebody_already_blocked_is_only_offered_the_way_back() {
        let offered = labels(&with(Flags::default(), &[], 0, person(true)));

        assert!(offered.contains(&"Unblock".to_string()));
        assert!(!offered.contains(&"Block…".to_string()));
    }

    /// A group has nobody to nickname and nobody to block.
    #[test]
    fn a_group_is_offered_neither() {
        let offered = labels(&menu(Flags::default(), &[], 0));

        assert!(!offered.contains(&"Set nickname…".to_string()));
        assert!(!offered.contains(&"Block…".to_string()));
        assert!(!offered.contains(&"Unblock".to_string()));
    }

    /// Blocking is destructive enough to be marked as such, and the ellipsis is
    /// the promise that it asks first -- the same two things the delete entry
    /// carries. Unblocking is neither: it is the undo.
    #[test]
    fn blocking_asks_first_and_unblocking_does_not() {
        let blocking = with(Flags::default(), &[], 0, person(false));
        assert!(blocking.iter().any(|item| matches!(
            item,
            Item::Entry { label, danger: true, .. } if label == "Block…"
        )));

        let unblocking = with(Flags::default(), &[], 0, person(true));
        assert!(unblocking.iter().any(|item| matches!(
            item,
            Item::Entry { label, danger: false, .. } if label == "Unblock"
        )));
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
