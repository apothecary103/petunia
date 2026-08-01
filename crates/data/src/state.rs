use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use uuid::Uuid;

/// Signal re-sends "started" roughly every ten seconds, so anything older than
/// this has stopped whether or not the "stopped" arrived.
const TYPING_TIMEOUT: Duration = Duration::from_secs(15);

use super::{Contact, Group, History, Index, Message, Thread};

/// Whether the message stream is up. Only the Signal worker knows; every view
/// reads it from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Connection {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
}

impl Connection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting",
            Self::Connected => "Connected",
            Self::Reconnecting => "Reconnecting",
        }
    }
}

/// Everything the panes read while rendering, so that adding a buffer kind does
/// not mean threading another parameter through every `view`.
pub struct State {
    pub aci: Uuid,
    pub contacts: Vec<Contact>,
    pub groups: Vec<Group>,
    pub index: Index,
    pub histories: HashMap<Thread, History>,
    /// Paths into the media cache; decoding is the view's business.
    pub avatars: HashMap<Thread, PathBuf>,
    /// Names from profiles. Signal's contact sync only carries names the user
    /// typed on their own phone, so for group members and anyone never saved this
    /// is the only name there is.
    profiles: HashMap<Uuid, String>,
    /// Contact names by uuid. The contact list is a `Vec` because order matters
    /// to the switcher, but resolving a name was a linear scan of it -- once per
    /// message, per mention and per reaction, every frame.
    named: HashMap<Uuid, String>,
    /// A nickname this account chose for someone, synced through Signal's
    /// Storage Service -- it outranks their contact-record and profile names
    /// the way a nickname on a real phone does. Keyed by uuid rather than
    /// carried on `Contact`, since it can exist for someone with no contact
    /// record at all.
    nicknames: HashMap<Uuid, String>,
    /// The note beside a nickname, shown but never used to resolve a name.
    pub notes: HashMap<Uuid, String>,
    /// Who this account has blocked. A set rather than a flag on `Contact` for
    /// the same reason a nickname is: somebody can be blocked without ever
    /// having been in the contact list.
    blocked: HashSet<Uuid>,
    pub connection: Connection,
    /// Who is currently typing, per thread. Kept here rather than in `History`
    /// because it is not part of the message stream.
    typing: HashMap<Thread, Vec<(Uuid, Instant)>>,
    pub sticker_packs: Vec<super::stickers::Pack>,
    /// Whether our own messages are attributed by name rather than as "You". A
    /// preference, pushed in from the config so that what to call someone stays
    /// one answer here rather than a decision each view makes for itself.
    pub show_own_name: bool,
}

impl State {
    pub fn new(aci: Uuid) -> Self {
        Self {
            aci,
            contacts: Vec::new(),
            groups: Vec::new(),
            index: Index::default(),
            histories: HashMap::new(),
            avatars: HashMap::new(),
            profiles: HashMap::new(),
            named: HashMap::new(),
            nicknames: HashMap::new(),
            notes: HashMap::new(),
            blocked: HashSet::new(),
            connection: Connection::default(),
            typing: HashMap::new(),
            sticker_packs: Vec::new(),
            show_own_name: false,
        }
    }

    /// The group behind a thread, for the details panel's member list.
    pub fn group(&self, thread: &Thread) -> Option<&Group> {
        let Thread::Group(master_key) = thread else {
            return None;
        };
        self.groups
            .iter()
            .find(|group| group.master_key == *master_key)
    }

    /// What the group calls someone, which is not their profile name: Signal
    /// lets members pick a short label and an emoji for themselves.
    pub fn member(&self, thread: &Thread, uuid: Uuid) -> Option<&super::Member> {
        self.group(thread)?
            .members
            .iter()
            .find(|member| member.uuid == uuid)
    }

    pub fn set_typing(&mut self, thread: &Thread, sender: Uuid, started: bool) {
        let entry = self.typing.entry(thread.clone()).or_default();
        entry.retain(|(who, _)| *who != sender);
        if started {
            entry.push((sender, Instant::now()));
        }
    }

    /// A "stopped" can be lost in transit, so an indicator also times out. Signal
    /// re-sends "started" about every ten seconds while typing continues.
    pub fn typing(&self, thread: &Thread) -> Vec<Uuid> {
        self.typing
            .get(thread)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|(_, since)| since.elapsed() < TYPING_TIMEOUT)
                    .map(|(who, _)| *who)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn anyone_typing(&self) -> bool {
        self.typing.keys().any(|thread| !self.typing(thread).is_empty())
    }

    /// Drops indicators that have timed out, so the tick can stop.
    pub fn expire_typing(&mut self) {
        for entries in self.typing.values_mut() {
            entries.retain(|(_, since)| since.elapsed() < TYPING_TIMEOUT);
        }
        self.typing.retain(|_, entries| !entries.is_empty());
    }

    /// Messages that are owed a read receipt: incoming, and not already read.
    pub fn unread_receipts(&self, thread: &Thread) -> Vec<(Uuid, u64)> {
        let Some(history) = self.history(thread) else {
            return Vec::new();
        };
        history
            .messages()
            .iter()
            .filter(|message| message.sender() != self.aci)
            .map(|message| (message.sender(), message.timestamp()))
            .collect()
    }

    pub fn history(&self, thread: &Thread) -> Option<&History> {
        self.histories.get(thread)
    }

    pub fn history_mut(&mut self, thread: &Thread) -> &mut History {
        self.histories.entry(thread.clone()).or_default()
    }

    pub fn avatar(&self, thread: &Thread) -> Option<&Path> {
        self.avatars.get(thread).map(PathBuf::as_path)
    }

    /// A group member's own avatar, which is keyed by their one-to-one thread
    /// even when the message arrived in a group.
    pub fn avatar_for(&self, sender: Uuid) -> Option<&Path> {
        self.avatars
            .get(&Thread::Contact(super::ContactId::Aci(sender)))
            .map(PathBuf::as_path)
    }

    pub fn title(&self, thread: &Thread) -> String {
        self.index
            .name(thread)
            .map(str::to_string)
            .unwrap_or_else(|| match thread {
                Thread::Contact(contact) => contact.uuid().to_string()[..8].to_string(),
                Thread::Group(_) => "Group".into(),
            })
    }

    pub fn sender_name(&self, sender: Uuid) -> String {
        if sender == self.aci && !self.show_own_name {
            return "You".into();
        }
        self.name_of(sender)
    }

    /// What to call someone: a nickname this account chose for them outranks
    /// everything, same as on a phone; then the name their contact record
    /// carries, then the one from their profile, then the front of their uuid.
    pub fn name_of(&self, uuid: Uuid) -> String {
        self.nicknames
            .get(&uuid)
            .or_else(|| self.named.get(&uuid))
            .or_else(|| self.profiles.get(&uuid))
            .cloned()
            .unwrap_or_else(|| uuid.to_string()[..8].to_string())
    }

    pub fn nickname_of(&self, uuid: Uuid) -> Option<&str> {
        self.nicknames.get(&uuid).map(String::as_str)
    }

    pub fn is_blocked(&self, uuid: Uuid) -> bool {
        self.blocked.contains(&uuid)
    }

    pub fn set_blocked(&mut self, uuid: Uuid, blocked: bool) {
        match blocked {
            true => self.blocked.insert(uuid),
            false => self.blocked.remove(&uuid),
        };
    }

    /// Sets or clears a nickname, straight from what Storage Service already
    /// carried or from what this account just set on it.
    pub fn set_nickname(&mut self, uuid: Uuid, name: Option<String>, note: Option<String>) {
        match name {
            Some(name) => self.nicknames.insert(uuid, name),
            None => self.nicknames.remove(&uuid),
        };
        match note {
            Some(note) => self.notes.insert(uuid, note),
            None => self.notes.remove(&uuid),
        };
        self.rename(uuid);
    }

    /// Our own name, for the sidebar's own row. Not "You": that reads as a label
    /// in a bubble, not as an identity.
    pub fn own_name(&self) -> String {
        self.name_of(self.aci)
    }

    pub fn set_profile(&mut self, uuid: Uuid, name: String) {
        self.profiles.insert(uuid, name);
        self.rename(uuid);
    }

    /// Brings a thread's sidebar name up to date with what we now know. Only the
    /// contact record outranks a profile, so anything else is replaced.
    fn rename(&mut self, uuid: Uuid) {
        let thread = Thread::Contact(super::ContactId::Aci(uuid));
        if self.index.get(&thread).is_none() {
            return;
        }
        let name = if uuid == self.aci {
            "Note to Self".to_string()
        } else {
            self.name_of(uuid)
        };
        self.index.set_name(&thread, name);
    }

    /// Records one person, without disturbing anybody else.
    ///
    /// Somebody found by username or phone number is somebody the contact sync
    /// has never heard of, and there is no second sync coming to carry them:
    /// `contacts_updated` rebuilds the whole list from what it is handed, so it
    /// is the wrong door for one arrival.
    pub fn record_contact(&mut self, contact: Contact) {
        if !contact.name.is_empty() {
            self.named.insert(contact.uuid, contact.name.clone());
        }
        match self
            .contacts
            .iter_mut()
            .find(|known| known.uuid == contact.uuid)
        {
            Some(known) => *known = contact,
            None => self.contacts.push(contact),
        }
    }

    pub fn contacts_updated(&mut self, contacts: Vec<Contact>, groups: Vec<Group>) {
        self.index.rebuild(&contacts, &groups, self.aci);
        self.named = contacts
            .iter()
            .filter(|contact| !contact.name.is_empty())
            .map(|contact| (contact.uuid, contact.name.clone()))
            .collect();
        self.contacts = contacts;
        self.groups = groups;
        // A contact sync rebuilds every name from the contact records alone, so
        // the profile names have to be laid back over the top.
        for uuid in self.profiles.keys().copied().collect::<Vec<_>>() {
            self.rename(uuid);
        }
    }

    pub fn record(&mut self, thread: &Thread, message: &Message) {
        self.record_preview(thread, crate::index::Preview::of(message));
    }

    /// The same, from a line read back off disk rather than from a message in
    /// hand. Which is how the sidebar survives a launch: what a row shows is
    /// written when the message arrives and read at startup, rather than being
    /// rebuilt from a scan that can come back empty.
    pub fn record_preview(&mut self, thread: &Thread, preview: crate::index::Preview) {
        // Resolved up front because `touch` borrows the index mutably.
        let name = self.thread_name(thread);
        self.index.touch(thread, preview, || name);
    }

    /// That a thread has something in it, with no line to show for it.
    pub fn record_activity(&mut self, thread: &Thread, at: u64) {
        let name = self.thread_name(thread);
        self.index.touch_activity(thread, at, || name);
    }

    fn thread_name(&self, thread: &Thread) -> String {
        match thread {
            Thread::Contact(contact) if contact.uuid() == self.aci => "Note to Self".to_string(),
            Thread::Contact(contact) => self.name_of(contact.uuid()),
            Thread::Group(_) => self
                .group(thread)
                .map(|group| group.title.clone())
                .unwrap_or_else(|| "Group".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContactId, MessageId};

    fn state() -> State {
        State::new(Uuid::new_v4())
    }

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::new_v4()))
    }

    #[test]
    fn typing_starts_and_stops() {
        let mut state = state();
        let thread = thread();
        let alice = Uuid::new_v4();

        state.set_typing(&thread, alice, true);
        assert_eq!(state.typing(&thread), [alice]);

        state.set_typing(&thread, alice, false);
        assert!(state.typing(&thread).is_empty());
    }

    #[test]
    fn a_repeated_start_does_not_duplicate_the_sender() {
        let mut state = state();
        let thread = thread();
        let alice = Uuid::new_v4();

        state.set_typing(&thread, alice, true);
        state.set_typing(&thread, alice, true);

        assert_eq!(state.typing(&thread).len(), 1);
    }

    #[test]
    fn several_people_can_type_at_once() {
        let mut state = state();
        let thread = thread();

        state.set_typing(&thread, Uuid::new_v4(), true);
        state.set_typing(&thread, Uuid::new_v4(), true);

        assert_eq!(state.typing(&thread).len(), 2);
    }

    /// A "stopped" can be lost in transit, so the indicator must age out on its
    /// own or it hangs there forever.
    #[test]
    fn a_stale_indicator_times_out() {
        let mut state = state();
        let thread = thread();
        state.set_typing(&thread, Uuid::new_v4(), true);

        let entries = state.typing.get_mut(&thread).unwrap();
        entries[0].1 = Instant::now() - TYPING_TIMEOUT * 2;

        assert!(state.typing(&thread).is_empty());
        assert!(!state.anyone_typing());
    }

    #[test]
    fn expiring_drops_the_thread_entirely() {
        let mut state = state();
        let thread = thread();
        state.set_typing(&thread, Uuid::new_v4(), true);
        state.typing.get_mut(&thread).unwrap()[0].1 = Instant::now() - TYPING_TIMEOUT * 2;

        state.expire_typing();

        assert!(state.typing.is_empty());
    }

    #[test]
    fn typing_in_another_thread_is_not_reported_here() {
        let mut state = state();
        let (a, b) = (thread(), thread());

        state.set_typing(&a, Uuid::new_v4(), true);

        assert!(state.typing(&b).is_empty());
        assert!(state.anyone_typing());
    }

    /// A contact record only carries a name if the user typed one on their phone,
    /// so a group member you have never saved has nothing but a uuid until their
    /// profile arrives. This is why every name showed up as eight hex digits.
    #[test]
    fn a_profile_names_someone_the_contact_sync_does_not() {
        let mut state = state();
        let stranger = Uuid::new_v4();

        assert_eq!(state.name_of(stranger), stranger.to_string()[..8]);

        state.set_profile(stranger, "Morgan".into());

        assert_eq!(state.name_of(stranger), "Morgan");
        assert_eq!(state.sender_name(stranger), "Morgan");
    }

    /// The name the user chose for a contact beats the one that contact chose for
    /// themselves, which is what every Signal client does.
    #[test]
    fn a_contact_name_outranks_a_profile_name() {
        let mut state = state();
        let alice = Uuid::new_v4();
        state.contacts_updated(
            vec![Contact {
                uuid: alice,
                name: "Alice at work".into(),
            }],
            Vec::new(),
        );

        state.set_profile(alice, "Alice".into());

        assert_eq!(state.name_of(alice), "Alice at work");
    }

    /// An empty contact name is what the sync actually sends for most people, and
    /// it must not win over a real profile name.
    #[test]
    fn an_empty_contact_name_loses_to_a_profile_name() {
        let mut state = state();
        let alice = Uuid::new_v4();
        state.contacts_updated(
            vec![Contact {
                uuid: alice,
                name: String::new(),
            }],
            Vec::new(),
        );
        state.set_profile(alice, "Alice".into());

        assert_eq!(state.name_of(alice), "Alice");
        assert_eq!(
            state
                .index
                .name(&Thread::Contact(crate::ContactId::Aci(alice))),
            Some("Alice")
        );
    }

    /// A contact sync rebuilds names from the contact records alone, so without
    /// re-applying them the sidebar silently reverts to uuid fragments.
    #[test]
    fn a_contact_sync_does_not_lose_profile_names() {
        let mut state = state();
        let alice = Uuid::new_v4();
        let nameless = vec![Contact {
            uuid: alice,
            name: String::new(),
        }];

        state.set_profile(alice, "Alice".into());
        state.contacts_updated(nameless.clone(), Vec::new());
        state.contacts_updated(nameless, Vec::new());

        assert_eq!(
            state
                .index
                .name(&Thread::Contact(crate::ContactId::Aci(alice))),
            Some("Alice")
        );
    }

    /// Our own row is an identity, not a label in a bubble: "You" belongs on a
    /// message, "Note to Self" on the thread.
    #[test]
    fn our_own_profile_names_us_without_renaming_note_to_self() {
        let mut state = state();
        let own = state.aci;
        state.contacts_updated(Vec::new(), Vec::new());

        state.set_profile(own, "Sam".into());

        assert_eq!(state.own_name(), "Sam");
        assert_eq!(state.sender_name(own), "You");
        assert_eq!(
            state.index.name(&Thread::Contact(crate::ContactId::Aci(own))),
            Some("Note to Self")
        );
    }

    /// The preference reaches the one place that decides what to call someone,
    /// so a mention, a quote and a run header cannot disagree about it.
    #[test]
    fn our_own_messages_can_be_attributed_by_name_instead() {
        let mut state = state();
        let own = state.aci;
        state.set_profile(own, "Sam".into());

        assert_eq!(state.sender_name(own), "You");

        state.show_own_name = true;

        assert_eq!(state.sender_name(own), "Sam");
    }

    /// Receipts are owed to other people, never to ourselves, or we would send
    /// read receipts for our own outgoing messages.
    #[test]
    fn read_receipts_exclude_our_own_messages() {
        let mut state = state();
        let thread = thread();
        let alice = Uuid::new_v4();
        let own = state.aci;

        let history = state.history_mut(&thread);
        history.insert(Message::plain(
            MessageId {
                timestamp: 1,
                sender: alice,
            },
            "hi".into(),
        ));
        history.insert(Message::plain(
            MessageId {
                timestamp: 2,
                sender: own,
            },
            "hello".into(),
        ));

        assert_eq!(state.unread_receipts(&thread), [(alice, 1)]);
    }

    #[test]
    fn an_unknown_thread_owes_no_receipts() {
        assert!(state().unread_receipts(&thread()).is_empty());
    }
}
