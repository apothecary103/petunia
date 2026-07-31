use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Contact, ContactId, Group, Message, Thread};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sort {
    #[default]
    Recent,
    Name,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Section {
    Pinned,
    Requests,
    /// A folder the user made. Flat: a conversation is in one or in none, which
    /// is as much structure as a chat list can carry before it needs a tree and
    /// a way to manage one.
    Folder(String),
    Chats,
    Archived,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    pub pinned: bool,
    pub archived: bool,
    pub blocked: bool,
    pub request: bool,
    /// A timestamp, not a duration: "muted for eight hours" has to survive a
    /// restart, and only an instant does.
    pub muted_until: Option<u64>,
    pub folder: Option<String>,
}

impl Flags {
    pub fn muted(&self, now: u64) -> bool {
        self.muted_until.is_some_and(|until| until > now)
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub thread: Thread,
    pub name: String,
    /// The name folded for comparison. Kept rather than computed, because the
    /// comparator runs on every reorder and every reorder runs on every message
    /// that arrives -- and `to_lowercase` allocates.
    sorted: String,
    pub preview: Option<Message>,
    pub unread: u32,
    pub mentions: u32,
    pub last_activity: u64,
    pub flags: Flags,
    pub note_to_self: bool,
}

#[derive(Debug, Default)]
pub struct Index {
    entries: Vec<Entry>,
    sort: Sort,
}

impl Entry {
    /// Archived beats everything -- putting something away is a decision about
    /// where it goes -- then pinned, then a folder, then the request queue.
    pub fn section(&self) -> Section {
        if self.flags.archived {
            Section::Archived
        } else if self.flags.pinned {
            Section::Pinned
        } else if let Some(folder) = self.flags.folder.clone() {
            Section::Folder(folder)
        } else if self.flags.request {
            Section::Requests
        } else {
            Section::Chats
        }
    }

    /// Whether there is a conversation here, as opposed to merely a contact. The
    /// contact store holds everyone the account has ever synced, and most of them
    /// have never exchanged a message and carry no profile name -- listing them
    /// fills the sidebar with uuid fragments. They stay reachable through the
    /// quick switcher, which is where starting a new conversation belongs.
    pub fn started(&self) -> bool {
        // Unread as well as preview: a thread that is owed attention must be
        // listed even if its preview never made it here.
        self.preview.is_some() || self.unread > 0 || self.note_to_self
    }
}

impl Index {
    /// Every known thread, conversation or not. Use `conversations` for anything
    /// that lists; this is for searching.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// What the sidebar shows: threads with something in them.
    pub fn conversations(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| entry.started())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, thread: &Thread) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.thread == *thread)
    }

    pub fn name(&self, thread: &Thread) -> Option<&str> {
        self.get(thread).map(|entry| entry.name.as_str())
    }

    pub fn section(&self, section: Section) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.section() == section)
    }

    /// The sections to draw, in order, with folders in between the fixed ones.
    /// Only the folders that have something in them: an empty folder is a name
    /// with nothing to show.
    pub fn sections(&self) -> Vec<Section> {
        let mut folders: Vec<&str> = self
            .conversations()
            .filter(|entry| entry.section() != Section::Archived)
            .filter_map(|entry| entry.flags.folder.as_deref())
            .collect();
        folders.sort_unstable();
        folders.dedup();

        let mut sections = vec![Section::Pinned, Section::Requests];
        sections.extend(folders.into_iter().map(|name| Section::Folder(name.to_owned())));
        sections.extend([Section::Chats, Section::Archived]);
        sections
    }

    /// Every folder that exists, for a menu offering to move something into one.
    pub fn folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = self
            .entries
            .iter()
            .filter_map(|entry| entry.flags.folder.clone())
            .collect();
        folders.sort_unstable();
        folders.dedup();
        folders
    }

    /// Applies what a menu chose, and records it nowhere: persistence is the
    /// store's business, because the index is not allowed to know about sqlite.
    pub fn set_flags(&mut self, thread: &Thread, flags: Flags) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.thread == *thread) {
            entry.flags = flags;
        }
        self.reorder();
    }

    pub fn flags(&self, thread: &Thread) -> Flags {
        self.get(thread).map(|entry| entry.flags.clone()).unwrap_or_default()
    }

    /// Forgets the conversation but keeps the contact. Everything `started` reads
    /// is cleared, so the sidebar stops listing it, while the person stays
    /// reachable through the quick switcher -- which is the difference between
    /// deleting a conversation and deleting someone.
    ///
    /// Note to Self is the exception, and stays listed: it is where the account
    /// itself lives rather than a conversation you chose to have.
    pub fn forget(&mut self, thread: &Thread) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.thread == *thread) {
            entry.preview = None;
            entry.unread = 0;
            entry.mentions = 0;
            entry.last_activity = 0;
            entry.flags = Flags {
                // Not a decision the conversation carried: whether someone is
                // blocked, or has never been let in, is about them.
                blocked: entry.flags.blocked,
                request: entry.flags.request,
                ..Flags::default()
            };
        }
        self.reorder();
    }

    /// Walks the sidebar order, wrapping at both ends. Archived threads are
    /// skipped so cycling never lands somewhere the sidebar is not showing.
    pub fn cycle(&self, from: Option<&Thread>, forward: bool) -> Option<&Thread> {
        let threads: Vec<&Thread> = self.selectable().map(|entry| &entry.thread).collect();
        if threads.is_empty() {
            return None;
        }

        let current = from.and_then(|thread| threads.iter().position(|other| *other == thread));
        let next = match (current, forward) {
            (Some(index), true) => (index + 1) % threads.len(),
            (Some(index), false) => (index + threads.len() - 1) % threads.len(),
            (None, true) => 0,
            (None, false) => threads.len() - 1,
        };
        Some(threads[next])
    }

    pub fn total_unread(&self) -> u32 {
        self.entries
            .iter()
            .filter(|entry| !entry.flags.archived && entry.flags.muted_until.is_none())
            .map(|entry| entry.unread)
            .sum()
    }

    pub fn next_unread(&self, from: Option<&Thread>) -> Option<&Thread> {
        let mut candidates = self.selectable().filter(|entry| entry.unread > 0);
        match from {
            None => candidates.next().map(|entry| &entry.thread),
            Some(thread) => {
                let ordered: Vec<&Entry> = candidates.collect();
                let start = ordered
                    .iter()
                    .position(|entry| entry.thread == *thread)
                    .map(|index| index + 1)
                    .unwrap_or_default();
                ordered
                    .get(start)
                    .or_else(|| ordered.first())
                    .map(|entry| &entry.thread)
            }
        }
    }

    pub fn set_sort(&mut self, sort: Sort) {
        self.sort = sort;
        self.reorder();
    }

    /// Rebuilds entries from a contact sync, preserving previews, unread counts
    /// and flags already learned for threads that survive.
    pub fn rebuild(&mut self, contacts: &[Contact], groups: &[Group], aci: Uuid) {
        let mut entries = Vec::with_capacity(contacts.len() + groups.len() + 1);

        let own = Thread::Contact(ContactId::Aci(aci));
        entries.push(self.carry(own, "Note to Self".into(), true));

        for contact in contacts {
            if contact.uuid == aci {
                continue;
            }
            let name = if contact.name.is_empty() {
                short(contact.uuid)
            } else {
                contact.name.clone()
            };
            entries.push(self.carry(Thread::Contact(ContactId::Aci(contact.uuid)), name, false));
        }

        for group in groups {
            let name = if group.title.is_empty() {
                "Group".into()
            } else {
                group.title.clone()
            };
            entries.push(self.carry(Thread::Group(group.master_key), name, false));
        }

        self.entries = entries;
        self.reorder();
    }

    /// Records activity for a thread the contact sync has not produced yet, so
    /// a message from an unknown sender still appears in the sidebar.
    pub fn touch(&mut self, thread: &Thread, message: &Message, name: impl FnOnce() -> String) {
        let newer = self
            .get(thread)
            .and_then(|entry| entry.preview.as_ref())
            .is_none_or(|current| current.timestamp() <= message.timestamp());

        match self.entries.iter_mut().find(|e| e.thread == *thread) {
            Some(entry) => {
                if newer {
                    entry.last_activity = message.timestamp();
                    entry.preview = Some(message.clone());
                }
            }
            None => {
                let name = name();
                self.entries.push(Entry {
                    thread: thread.clone(),
                    sorted: name.to_lowercase(),
                    name,
                    last_activity: message.timestamp(),
                    preview: Some(message.clone()),
                    unread: 0,
                    mentions: 0,
                    flags: Flags::default(),
                    note_to_self: false,
                });
            }
        }
        self.reorder();
    }

    pub fn mark_unread(&mut self, thread: &Thread, mentioned: bool) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.thread == *thread) {
            entry.unread += 1;
            if mentioned {
                entry.mentions += 1;
            }
        }
    }

    pub fn clear_unread(&mut self, thread: &Thread) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.thread == *thread) {
            entry.unread = 0;
            entry.mentions = 0;
        }
    }

    pub fn set_unread(&mut self, thread: &Thread, unread: u32) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.thread == *thread) {
            entry.unread = unread;
        }
    }

    /// Names arrive after the entry does -- a profile fetch is a round trip -- so
    /// an entry's name has to be replaceable in place.
    pub fn set_name(&mut self, thread: &Thread, name: String) {
        let Some(entry) = self.entries.iter_mut().find(|e| e.thread == *thread) else {
            return;
        };
        if entry.name == name {
            return;
        }
        entry.sorted = name.to_lowercase();
        entry.name = name;
        self.reorder();
    }

    /// What the keyboard walks: exactly what the sidebar lists, so cycling never
    /// lands somewhere the eye cannot follow.
    fn selectable(&self) -> impl Iterator<Item = &Entry> {
        self.conversations().filter(|entry| !entry.flags.archived)
    }

    fn carry(&self, thread: Thread, name: String, note_to_self: bool) -> Entry {
        match self.get(&thread) {
            Some(existing) => Entry {
                sorted: name.to_lowercase(),
                name,
                note_to_self,
                ..existing.clone()
            },
            None => Entry {
                thread,
                sorted: name.to_lowercase(),
                name,
                preview: None,
                unread: 0,
                mentions: 0,
                last_activity: 0,
                flags: Flags::default(),
                note_to_self,
            },
        }
    }

    fn reorder(&mut self) {
        match self.sort {
            Sort::Recent => self.entries.sort_by(|a, b| {
                b.last_activity
                    .cmp(&a.last_activity)
                    .then_with(|| a.sorted.cmp(&b.sorted))
            }),
            Sort::Name => self.entries.sort_by(|a, b| a.sorted.cmp(&b.sorted)),
        }
    }
}

fn short(uuid: Uuid) -> String {
    uuid.to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::MessageId;

    fn contact(name: &str) -> Contact {
        Contact {
            uuid: Uuid::new_v4(),
            name: name.into(),
        }
    }

    fn group(title: &str) -> Group {
        Group {
            master_key: [u8::try_from(title.len()).unwrap_or_default(); 32],
            title: title.into(),
            description: None,
            members: Vec::new(),
            invited: 0,
            requesting: 0,
            expire_timer: None,
        }
    }

    fn message(timestamp: u64, sender: Uuid, body: &str) -> Message {
        Message::plain(MessageId { timestamp, sender }, body.into())
    }

    fn index(contacts: &[Contact], groups: &[Group]) -> (Index, Uuid) {
        let aci = Uuid::new_v4();
        let mut index = Index::default();
        index.rebuild(contacts, groups, aci);
        (index, aci)
    }

    fn names(index: &Index) -> Vec<&str> {
        index.entries().iter().map(|e| e.name.as_str()).collect()
    }

    fn unknown() -> String {
        "Unknown".into()
    }

    /// The threads the sidebar would actually draw.
    fn listed(index: &Index) -> Vec<Thread> {
        index.conversations().map(|e| e.thread.clone()).collect()
    }

    #[test]
    fn always_includes_note_to_self() {
        let (index, aci) = index(&[], &[]);
        let own = Thread::Contact(ContactId::Aci(aci));

        assert!(index.get(&own).unwrap().note_to_self);
        assert_eq!(names(&index), ["Note to Self"]);
    }

    #[test]
    fn does_not_duplicate_self_from_the_contact_list() {
        let aci = Uuid::new_v4();
        let mut index = Index::default();
        let own = Contact {
            uuid: aci,
            name: "Me".into(),
        };

        index.rebuild(&[own, contact("Alice")], &[], aci);

        assert_eq!(index.entries().len(), 2);
        assert_eq!(index.get(&Thread::Contact(ContactId::Aci(aci))).unwrap().name, "Note to Self");
    }

    #[test]
    fn includes_contacts_and_groups() {
        let (index, _) = index(&[contact("Alice")], &[group("Devs")]);

        assert_eq!(index.entries().len(), 3);
        assert!(names(&index).contains(&"Alice"));
        assert!(names(&index).contains(&"Devs"));
    }

    #[test]
    fn falls_back_to_a_short_uuid_for_a_nameless_contact() {
        let nameless = contact("");
        let (index, _) = index(std::slice::from_ref(&nameless), &[]);

        assert!(names(&index).contains(&&nameless.uuid.to_string()[..8]));
    }

    /// A synced contact with no messages is not a conversation. The store holds
    /// hundreds of them, most with no name, and listing them made the sidebar a
    /// wall of uuid fragments.
    #[test]
    fn only_threads_with_messages_are_conversations() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, _) = index(&[alice.clone(), contact("")], &[group("Devs")]);

        assert_eq!(index.conversations().count(), 1, "note to self only");

        index.touch(&thread, &message(100, alice.uuid, "hi"), unknown);

        let listed: Vec<_> = index.conversations().map(|e| e.name.as_str()).collect();
        assert_eq!(listed, ["Alice", "Note to Self"]);
    }

    /// Note to Self is always there to be written in, even before it has anything
    /// in it -- it is the one thread you can start without finding anyone.
    #[test]
    fn note_to_self_counts_as_a_conversation_while_empty() {
        let (index, aci) = index(&[], &[]);

        assert!(
            index
                .get(&Thread::Contact(ContactId::Aci(aci)))
                .unwrap()
                .started()
        );
    }

    /// The switcher searches everything, so an unlisted contact is still one
    /// keystroke away.
    #[test]
    fn every_contact_is_still_searchable() {
        let (index, _) = index(&[contact("Alice")], &[]);

        assert_eq!(index.entries().len(), 2);
        assert!(index.entries().iter().any(|entry| entry.name == "Alice"));
    }

    #[test]
    fn cycling_skips_threads_the_sidebar_does_not_list() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, aci) = index(&[alice.clone(), contact("Bob")], &[]);
        index.touch(&thread, &message(100, alice.uuid, "hi"), unknown);

        let walked: Vec<_> = index.conversations().map(|e| e.thread.clone()).collect();

        assert_eq!(walked.len(), 2);
        assert!(walked.contains(&thread));
        assert!(walked.contains(&Thread::Contact(ContactId::Aci(aci))));
    }

    #[test]
    fn sorts_by_recency_then_name() {
        let alice = contact("Alice");
        let bob = contact("Bob");
        let (mut index, _) = index(&[alice.clone(), bob.clone()], &[]);

        index.touch(
            &Thread::Contact(ContactId::Aci(bob.uuid)),
            &message(500, bob.uuid, "later"),
            unknown,
        );

        assert_eq!(names(&index)[0], "Bob");
    }

    #[test]
    fn sorts_by_name_when_asked() {
        let (mut index, _) = index(&[contact("Zoe"), contact("Alice")], &[]);
        index.set_sort(Sort::Name);

        assert_eq!(names(&index), ["Alice", "Note to Self", "Zoe"]);
    }

    /// The point of `forget`: the conversation goes, the contact stays. Deleting
    /// a chat is not deleting the person, and they have to stay reachable through
    /// the quick switcher to start another one.
    #[test]
    fn forgetting_a_conversation_keeps_the_contact() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, _) = index(std::slice::from_ref(&alice), &[]);
        index.touch(&thread, &message(100, alice.uuid, "hi"), unknown);
        index.set_flags(
            &thread,
            Flags {
                pinned: true,
                folder: Some("Work".into()),
                ..Default::default()
            },
        );
        assert!(index.get(&thread).expect("listed").started());

        index.forget(&thread);

        let entry = index.get(&thread).expect("still a contact");
        assert!(!entry.started(), "no longer a conversation");
        assert_eq!(entry.name, "Alice");
        assert!(entry.preview.is_none());
        assert_eq!(entry.unread, 0);
        // The flags went with it, or the emptied conversation would still be
        // pinned to the top of a list it is no longer in.
        assert!(!entry.flags.pinned);
        assert_eq!(entry.flags.folder, None);
        assert!(!listed(&index).contains(&thread));
    }

    /// Whether someone is blocked, or has never been let in, is a fact about them
    /// rather than about the conversation -- so it survives.
    #[test]
    fn forgetting_keeps_what_is_true_about_the_person() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, _) = index(std::slice::from_ref(&alice), &[]);
        index.touch(&thread, &message(100, alice.uuid, "hi"), unknown);
        index.set_flags(
            &thread,
            Flags {
                blocked: true,
                request: true,
                pinned: true,
                ..Default::default()
            },
        );

        index.forget(&thread);

        let flags = index.flags(&thread);
        assert!(flags.blocked);
        assert!(flags.request);
        assert!(!flags.pinned);
    }

    /// Forgetting one conversation must not empty another.
    #[test]
    fn forgetting_leaves_every_other_conversation_alone() {
        let (alice, bob) = (contact("Alice"), contact("Bob"));
        let (mut index, _) = index(&[alice.clone(), bob.clone()], &[]);
        let (a, b) = (
            Thread::Contact(ContactId::Aci(alice.uuid)),
            Thread::Contact(ContactId::Aci(bob.uuid)),
        );
        index.touch(&a, &message(100, alice.uuid, "hi"), unknown);
        index.touch(&b, &message(200, bob.uuid, "hello"), unknown);

        index.forget(&a);

        assert!(!listed(&index).contains(&a));
        assert!(listed(&index).contains(&b));
        assert_eq!(index.get(&b).expect("kept").last_activity, 200);
    }

    /// Note to Self is where the account lives rather than a conversation you
    /// chose to have, so emptying it does not take it off the list.
    #[test]
    fn forgetting_note_to_self_leaves_it_listed() {
        let (mut index, aci) = index(&[], &[]);
        let own = Thread::Contact(ContactId::Aci(aci));
        index.touch(&own, &message(100, aci, "a note"), unknown);

        index.forget(&own);

        assert!(index.get(&own).expect("listed").started());
        assert!(index.get(&own).expect("listed").preview.is_none());
    }

    #[test]
    fn touch_keeps_only_the_newest_preview() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, _) = index(std::slice::from_ref(&alice), &[]);

        index.touch(&thread, &message(200, alice.uuid, "newer"), unknown);
        index.touch(&thread, &message(100, alice.uuid, "older"), unknown);

        let entry = index.get(&thread).unwrap();
        assert_eq!(entry.preview.as_ref().unwrap().text(), Some("newer"));
        assert_eq!(entry.last_activity, 200);
    }

    #[test]
    fn touch_adds_an_unknown_thread() {
        let (mut index, _) = index(&[], &[]);
        let stranger = Uuid::new_v4();
        let thread = Thread::Contact(ContactId::Aci(stranger));

        index.touch(&thread, &message(100, stranger, "hello"), unknown);

        assert_eq!(index.get(&thread).unwrap().name, "Unknown");
    }

    #[test]
    fn rebuild_preserves_unread_and_previews() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, aci) = index(std::slice::from_ref(&alice), &[]);

        index.touch(&thread, &message(100, alice.uuid, "hi"), unknown);
        index.mark_unread(&thread, false);
        index.rebuild(std::slice::from_ref(&alice), &[], aci);

        let entry = index.get(&thread).unwrap();
        assert_eq!(entry.unread, 1);
        assert_eq!(entry.preview.as_ref().unwrap().text(), Some("hi"));
    }

    #[test]
    fn assigns_sections_from_flags() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, _) = index(&[alice], &[]);

        assert_eq!(index.get(&thread).unwrap().section(), Section::Chats);

        index.set_flags(
            &thread,
            Flags {
                pinned: true,
                ..Flags::default()
            },
        );
        assert_eq!(index.get(&thread).unwrap().section(), Section::Pinned);

        index.set_flags(
            &thread,
            Flags {
                pinned: true,
                archived: true,
                ..Flags::default()
            },
        );
        assert_eq!(index.get(&thread).unwrap().section(), Section::Archived);
    }

    #[test]
    fn total_unread_skips_archived_and_muted() {
        let loud = contact("Loud");
        let muted = contact("Muted");
        let archived = contact("Archived");
        let (mut index, _) = index(&[loud.clone(), muted.clone(), archived.clone()], &[]);

        for who in [&loud, &muted, &archived] {
            index.mark_unread(&Thread::Contact(ContactId::Aci(who.uuid)), false);
        }
        index.set_flags(
            &Thread::Contact(ContactId::Aci(muted.uuid)),
            Flags {
                muted_until: Some(u64::MAX),
                ..Flags::default()
            },
        );
        index.set_flags(
            &Thread::Contact(ContactId::Aci(archived.uuid)),
            Flags {
                archived: true,
                ..Flags::default()
            },
        );

        assert_eq!(index.total_unread(), 1);
    }

    #[test]
    fn cycles_forward_and_backward_with_wrapping() {
        let (alice, bob) = (contact("Alice"), contact("Bob"));
        let (mut index, _) = index(&[alice.clone(), bob.clone()], &[]);
        for who in [&alice, &bob] {
            index.touch(
                &Thread::Contact(ContactId::Aci(who.uuid)),
                &message(100, who.uuid, "hi"),
                unknown,
            );
        }
        index.set_sort(Sort::Name);

        let order: Vec<_> = index.conversations().map(|e| e.thread.clone()).collect();
        let (first, last) = (order[0].clone(), order[order.len() - 1].clone());

        assert_eq!(index.cycle(Some(&first), true), Some(&order[1]));
        assert_eq!(index.cycle(Some(&last), true), Some(&first));
        assert_eq!(index.cycle(Some(&first), false), Some(&last));
    }

    #[test]
    fn cycling_an_empty_index_yields_nothing() {
        let index = Index::default();
        assert_eq!(index.cycle(None, true), None);
    }

    #[test]
    fn cycling_skips_archived_threads() {
        let alice = contact("Alice");
        let (mut index, _) = index(std::slice::from_ref(&alice), &[]);
        index.set_sort(Sort::Name);
        index.set_flags(
            &Thread::Contact(ContactId::Aci(alice.uuid)),
            Flags {
                archived: true,
                ..Flags::default()
            },
        );

        let threads: Vec<_> = index.conversations().collect();
        assert_eq!(threads.len(), 1);
        assert!(!threads[0].flags.archived);
    }

    #[test]
    fn finds_the_next_unread_thread() {
        let alice = contact("Alice");
        let bob = contact("Bob");
        let (mut index, _) = index(&[alice.clone(), bob.clone()], &[]);
        index.set_sort(Sort::Name);

        let alice_thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let bob_thread = Thread::Contact(ContactId::Aci(bob.uuid));
        index.mark_unread(&alice_thread, false);
        index.mark_unread(&bob_thread, false);

        assert_eq!(index.next_unread(None), Some(&alice_thread));
        assert_eq!(index.next_unread(Some(&alice_thread)), Some(&bob_thread));
        assert_eq!(index.next_unread(Some(&bob_thread)), Some(&alice_thread));
    }

    #[test]
    fn clear_unread_resets_mentions_too() {
        let alice = contact("Alice");
        let thread = Thread::Contact(ContactId::Aci(alice.uuid));
        let (mut index, _) = index(&[alice], &[]);

        index.mark_unread(&thread, true);
        assert_eq!(index.get(&thread).unwrap().mentions, 1);

        index.clear_unread(&thread);
        assert_eq!(index.get(&thread).unwrap().unread, 0);
        assert_eq!(index.get(&thread).unwrap().mentions, 0);
    }
}
