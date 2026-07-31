use uuid::Uuid;

use super::attachment;
use super::message::project;
use super::{Message, MessageId, Reaction, Status};

#[derive(Debug, Default)]
pub struct History {
    messages: Vec<Message>,
    oldest: Option<u64>,
    more: bool,
    loading: bool,
    /// Where the "new" divider sits. Pinned when the thread is opened rather
    /// than recomputed, so it does not jump as messages are read.
    first_unread: Option<u64>,
}

impl History {
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn last(&self) -> Option<&Message> {
        self.messages.last()
    }

    pub fn has_more(&self) -> bool {
        self.more
    }

    pub fn oldest(&self) -> Option<u64> {
        self.oldest
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    pub fn first_unread(&self) -> Option<u64> {
        self.first_unread
    }

    /// Called when the thread is opened with unread messages, so the divider
    /// stays put while the reader catches up.
    pub fn mark_unread_from(&mut self, timestamp: Option<u64>) {
        self.first_unread = timestamp;
    }

    pub fn find(&self, id: &MessageId) -> Option<&Message> {
        self.index_of(id).map(|index| &self.messages[index])
    }

    /// Merges a freshly loaded page over what is already held, so that the
    /// optimistic rows written on send are not lost when history arrives.
    pub fn merge(&mut self, page: Vec<Message>, more: bool) {
        let live = std::mem::replace(&mut self.messages, page);
        for message in live {
            match self.index_of(&message.id) {
                Some(index) => self.messages[index] = keep_newer(&self.messages[index], message),
                None => self.messages.push(message),
            }
        }
        self.sort();
        self.more = more;
        self.loading = false;
        self.oldest = self.messages.first().map(Message::timestamp);
    }

    pub fn prepend(&mut self, older: Vec<Message>, more: bool) {
        for message in older {
            if self.index_of(&message.id).is_none() {
                self.messages.push(message);
            }
        }
        self.sort();
        self.more = more;
        self.loading = false;
        self.oldest = self.messages.first().map(Message::timestamp);
    }

    pub fn insert(&mut self, message: Message) {
        match self.index_of(&message.id) {
            Some(index) => self.messages[index] = keep_newer(&self.messages[index], message),
            None => {
                let append = self
                    .messages
                    .last()
                    .is_none_or(|last| last.id <= message.id);
                self.messages.push(message);
                if !append {
                    self.sort();
                }
            }
        }
    }

    pub fn apply_edit(&mut self, target: &MessageId, edit: Message, edited_at: u64) {
        if let Some(index) = self.index_of(target) {
            project::apply_edit(&mut self.messages[index], edit, edited_at);
        }
    }

    pub fn apply_reaction(&mut self, target: &MessageId, reaction: Reaction, remove: bool) {
        if let Some(index) = self.index_of(target) {
            project::apply_reaction(&mut self.messages[index], reaction, remove);
        }
    }

    /// A digest is shared by every copy of the same bytes, so a single download
    /// resolves the attachment wherever it was forwarded to.
    pub fn set_blob(
        &mut self,
        id: &attachment::Id,
        blob: attachment::Blob,
        measured: Option<attachment::Size>,
    ) {
        for message in &mut self.messages {
            message.set_blob(id, blob.clone());
            if let Some(measured) = measured {
                message.set_image_size(id, measured);
            }
        }
    }

    pub fn set_poster(&mut self, id: &attachment::Id, path: std::path::PathBuf) {
        for message in &mut self.messages {
            message.set_poster(id, path.clone());
        }
    }

    pub fn apply_delete(&mut self, target: &MessageId) {
        if let Some(index) = self.index_of(target) {
            project::apply_delete(&mut self.messages[index]);
        }
    }

    /// Receipts name a timestamp but not a thread, so callers scan every
    /// history; statuses only ever move forwards.
    pub fn apply_status(&mut self, timestamps: &[u64], aci: Uuid, status: Status) {
        for message in self
            .messages
            .iter_mut()
            .filter(|message| message.sender() == aci)
        {
            if timestamps.contains(&message.timestamp())
                && message.status.is_none_or(|current| current < status)
            {
                message.status = Some(status);
            }
        }
    }

    fn index_of(&self, id: &MessageId) -> Option<usize> {
        self.messages.iter().position(|message| message.id == *id)
    }

    fn sort(&mut self) {
        self.messages.sort_by_key(|message| message.id);
    }
}

/// A stored row wins over an optimistic echo, but never loses a status the
/// echo already carries.
fn keep_newer(existing: &Message, incoming: Message) -> Message {
    let status = existing.status.max(incoming.status);
    Message {
        status,
        ..incoming
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Content;

    fn message(timestamp: u64, sender: Uuid, body: &str) -> Message {
        Message::plain(MessageId { timestamp, sender }, body.into())
    }

    fn bodies(history: &History) -> Vec<&str> {
        history
            .messages()
            .iter()
            .filter_map(Message::text)
            .collect()
    }

    #[test]
    fn keeps_messages_sorted_on_insert() {
        let sender = Uuid::new_v4();
        let mut history = History::default();

        history.insert(message(300, sender, "c"));
        history.insert(message(100, sender, "a"));
        history.insert(message(200, sender, "b"));

        assert_eq!(bodies(&history), ["a", "b", "c"]);
    }

    #[test]
    fn replaces_a_message_with_the_same_id() {
        let sender = Uuid::new_v4();
        let mut history = History::default();

        history.insert(message(100, sender, "first"));
        history.insert(message(100, sender, "second"));

        assert_eq!(bodies(&history), ["second"]);
    }

    #[test]
    fn merge_keeps_live_messages_missing_from_the_page() {
        let sender = Uuid::new_v4();
        let mut history = History::default();
        history.insert(message(300, sender, "optimistic"));

        history.merge(vec![message(100, sender, "stored")], false);

        assert_eq!(bodies(&history), ["stored", "optimistic"]);
    }

    #[test]
    fn merge_does_not_regress_a_status_the_echo_already_has() {
        let sender = Uuid::new_v4();
        let mut history = History::default();
        let mut sent = message(100, sender, "hi");
        sent.status = Some(Status::Read);
        history.insert(sent);

        let mut stored = message(100, sender, "hi");
        stored.status = Some(Status::Sent);
        history.merge(vec![stored], false);

        assert_eq!(history.messages()[0].status, Some(Status::Read));
    }

    #[test]
    fn merge_records_paging_state() {
        let sender = Uuid::new_v4();
        let mut history = History::default();
        history.set_loading(true);

        history.merge(vec![message(100, sender, "a")], true);

        assert!(history.has_more());
        assert!(!history.is_loading());
        assert_eq!(history.oldest(), Some(100));
    }

    #[test]
    fn prepend_adds_older_messages_without_duplicating() {
        let sender = Uuid::new_v4();
        let mut history = History::default();
        history.merge(vec![message(200, sender, "b")], true);

        history.prepend(
            vec![message(100, sender, "a"), message(200, sender, "b")],
            false,
        );

        assert_eq!(bodies(&history), ["a", "b"]);
        assert!(!history.has_more());
        assert_eq!(history.oldest(), Some(100));
    }

    #[test]
    fn upgrades_status_only_for_own_messages() {
        let own = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut history = History::default();
        history.insert(message(100, own, "mine"));
        history.insert(message(200, other, "theirs"));

        history.apply_status(&[100, 200], own, Status::Delivered);

        assert_eq!(history.messages()[0].status, Some(Status::Delivered));
        assert_eq!(history.messages()[1].status, None);
    }

    #[test]
    fn never_moves_a_status_backwards() {
        let own = Uuid::new_v4();
        let mut history = History::default();
        history.insert(message(100, own, "mine"));

        history.apply_status(&[100], own, Status::Read);
        history.apply_status(&[100], own, Status::Delivered);

        assert_eq!(history.messages()[0].status, Some(Status::Read));
    }

    #[test]
    fn applies_a_reaction_to_its_target() {
        let sender = Uuid::new_v4();
        let reactor = Uuid::new_v4();
        let id = MessageId {
            timestamp: 100,
            sender,
        };
        let mut history = History::default();
        history.insert(message(100, sender, "nice"));

        history.apply_reaction(
            &id,
            Reaction {
                author: reactor,
                emoji: "👍".into(),
                timestamp: 200,
            },
            false,
        );

        assert_eq!(history.find(&id).unwrap().reactions.len(), 1);
    }

    #[test]
    fn applies_an_edit_and_a_delete() {
        let sender = Uuid::new_v4();
        let id = MessageId {
            timestamp: 100,
            sender,
        };
        let mut history = History::default();
        history.insert(message(100, sender, "typo"));

        history.apply_edit(&id, message(150, sender, "fixed"), 150);
        assert_eq!(history.find(&id).unwrap().text(), Some("fixed"));
        assert_eq!(history.find(&id).unwrap().edited, Some(150));

        history.apply_delete(&id);
        assert!(matches!(
            history.find(&id).unwrap().content,
            Content::Deleted
        ));
    }

}
