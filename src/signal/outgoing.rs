use presage::libsignal_service::content::{
    Content, ContentBody, DataMessage, GroupContextV2, Metadata,
};
use presage::libsignal_service::proto::{
    AttachmentPointer, EditMessage, ReceiptMessage, SyncMessage, TypingMessage, data_message,
    receipt_message, sync_message, typing_message,
};
use presage::libsignal_service::zkgroup::groups::{GroupMasterKey, GroupSecretParams};
use presage::libsignal_service::protocol::{DeviceId, ServiceId};
use uuid::Uuid;

use crate::data::message::{Range, range};
use crate::data::{MessageId, Thread};

pub fn text(thread: &Thread, body: String, timestamp: u64) -> DataMessage {
    message(thread, body, Vec::new(), timestamp)
}

/// The single builder for anything petunia sends. Both the wire message and the
/// row saved for the optimistic echo come from one value, so the two cannot
/// drift as attachments, reactions and edits are added.
pub fn message(
    thread: &Thread,
    body: String,
    attachments: Vec<AttachmentPointer>,
    timestamp: u64,
) -> DataMessage {
    DataMessage {
        body: Some(body).filter(|body| !body.is_empty()),
        attachments,
        timestamp: Some(timestamp),
        group_v2: group_context(thread),
        ..Default::default()
    }
}

/// Attaches a reply to a message that has already been built, so the reply path
/// does not need a second builder that could drift from this one.
pub fn replying_to(mut message: DataMessage, quote: data_message::Quote) -> DataMessage {
    message.quote = Some(quote);
    message
}

/// A reply carries a snapshot of what it quotes, because the recipient may not
/// have the original. `author_aci_binary` is what current clients read; the
/// string form is sent too so older ones still resolve the author.
pub fn quote(target: &MessageId, body: &str, ranges: &[Range]) -> data_message::Quote {
    data_message::Quote {
        id: Some(target.timestamp),
        author_aci: Some(target.sender.to_string()),
        author_aci_binary: Some(target.sender.as_bytes().to_vec()),
        text: Some(body.to_string()),
        body_ranges: range::to_proto(body, ranges),
        r#type: Some(data_message::quote::Type::Normal as i32),
        attachments: Vec::new(),
    }
}

pub fn reaction(
    thread: &Thread,
    target: &MessageId,
    emoji: String,
    remove: bool,
    timestamp: u64,
) -> DataMessage {
    DataMessage {
        reaction: Some(data_message::Reaction {
            emoji: Some(emoji),
            remove: Some(remove),
            target_author_aci: Some(target.sender.to_string()),
            target_author_aci_binary: Some(target.sender.as_bytes().to_vec()),
            target_sent_timestamp: Some(target.timestamp),
        }),
        timestamp: Some(timestamp),
        group_v2: group_context(thread),
        ..Default::default()
    }
}

/// A remote delete. Signal only lets you delete your own messages, which the
/// caller enforces; the recipient tombstones the row either way.
pub fn delete(thread: &Thread, target: u64, timestamp: u64) -> DataMessage {
    DataMessage {
        delete: Some(data_message::Delete {
            target_sent_timestamp: Some(target),
        }),
        timestamp: Some(timestamp),
        group_v2: group_context(thread),
        ..Default::default()
    }
}

/// The edit's own timestamp orders revisions; `target_sent_timestamp` keeps it
/// attached to the original, which is also how presage derives its identity.
pub fn edit(thread: &Thread, target: u64, body: String, timestamp: u64) -> EditMessage {
    EditMessage {
        target_sent_timestamp: Some(target),
        data_message: Some(message(thread, body, Vec::new(), timestamp)),
    }
}

/// presage sends neither delivery nor read receipts, so both are ours to build.
pub fn receipt(kind: receipt_message::Type, timestamps: Vec<u64>) -> ReceiptMessage {
    ReceiptMessage {
        r#type: Some(kind as i32),
        timestamp: timestamps,
    }
}

/// Tells our other devices what we have read here, so the unread count does not
/// come straight back on the next sync.
pub fn read_sync(reads: &[(Uuid, u64)]) -> SyncMessage {
    SyncMessage {
        read: reads
            .iter()
            .map(|(sender, timestamp)| sync_message::Read {
                sender_aci: Some(sender.to_string()),
                sender_aci_binary: Some(sender.as_bytes().to_vec()),
                timestamp: Some(*timestamp),
            })
            .collect(),
        ..Default::default()
    }
}

/// The group id a typing indicator carries is the *identifier* derived from the
/// master key, not the master key itself.
pub fn typing(thread: &Thread, started: bool, timestamp: u64) -> TypingMessage {
    let action = if started {
        typing_message::Action::Started
    } else {
        typing_message::Action::Stopped
    };

    TypingMessage {
        timestamp: Some(timestamp),
        action: Some(action as i32),
        group_id: match thread {
            Thread::Contact(_) => None,
            Thread::Group(master_key) => group_identifier(master_key),
        },
    }
}

fn group_identifier(master_key: &[u8; 32]) -> Option<Vec<u8>> {
    let key = GroupMasterKey::new(*master_key);
    Some(
        GroupSecretParams::derive_from_master_key(key)
            .get_group_identifier()
            .to_vec(),
    )
}

/// Wraps an outgoing message as presage would have stored it had it come off
/// the wire, so the send shows up before the server answers.
pub fn envelope(
    aci: Uuid,
    device: DeviceId,
    thread: &Thread,
    message: impl Into<ContentBody>,
    timestamp: u64,
) -> Content {
    let time = chrono::DateTime::from_timestamp_millis(timestamp as i64).unwrap_or_default();
    let destination = match thread {
        Thread::Contact(contact) => contact.into(),
        Thread::Group(_) => ServiceId::Aci(aci.into()),
    };

    Content::from_body(
        message,
        Metadata {
            sender: ServiceId::Aci(aci.into()),
            destination,
            sender_device: device,
            timestamp: time,
            server_timestamp: time,
            needs_receipt: false,
            unidentified_sender: false,
            was_plaintext: false,
            server_guid: None,
        },
    )
}

fn group_context(thread: &Thread) -> Option<GroupContextV2> {
    match thread {
        Thread::Contact(_) => None,
        Thread::Group(master_key) => Some(GroupContextV2 {
            master_key: Some(master_key.to_vec()),
            revision: Some(0),
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ContactId;
    use crate::data::message::project::{Fragment, classify, from_content};

    #[test]
    fn a_contact_message_carries_no_group_context() {
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));
        assert!(text(&thread, "hi".into(), 1).group_v2.is_none());
    }

    #[test]
    fn a_group_message_carries_its_master_key() {
        let master_key = [9u8; 32];
        let message = text(&Thread::Group(master_key), "hi".into(), 1);

        let context = message.group_v2.unwrap();
        assert_eq!(context.master_key.unwrap(), master_key.to_vec());
    }

    #[test]
    fn the_echoed_row_projects_back_to_the_message_that_was_sent() {
        let aci = Uuid::new_v4();
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));
        let message = text(&thread, "hello".into(), 1234);

        let content = envelope(aci, DeviceId::new(1).unwrap(), &thread, message, 1234);
        let (_, projected) = from_content(&content).unwrap();

        assert_eq!(projected.text(), Some("hello"));
        assert_eq!(projected.timestamp(), 1234);
        assert_eq!(projected.sender(), aci);
    }

    /// A bare DataMessage has no destination field, so presage derives the thread
    /// from the envelope sender -- which for our own echo is us. The thread has to
    /// come from the caller, which is why save_outgoing passes it to the store and
    /// why history loading keys pages on the thread rather than on the content.
    #[test]
    fn a_contact_echo_cannot_have_its_thread_derived_from_the_content() {
        let aci = Uuid::new_v4();
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));
        let message = text(&thread, "hello".into(), 1234);

        let content = envelope(aci, DeviceId::new(1).unwrap(), &thread, message, 1234);
        let (derived, _) = from_content(&content).unwrap();

        assert_eq!(derived, Thread::Contact(ContactId::Aci(aci)));
        assert_ne!(derived, thread);
    }

    fn target() -> MessageId {
        MessageId {
            timestamp: 500,
            sender: Uuid::new_v4(),
        }
    }

    fn echo(thread: &Thread, body: impl Into<ContentBody>) -> Fragment {
        let content = envelope(
            Uuid::new_v4(),
            DeviceId::new(1).unwrap(),
            thread,
            body,
            900,
        );
        classify(&content).expect("a classified fragment").1
    }

    #[test]
    fn a_reaction_round_trips_to_the_message_it_targets() {
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));
        let target = target();

        let built = reaction(&thread, &target, "🎉".into(), false, 900);

        let Fragment::Reaction {
            target: parsed,
            reaction,
            remove,
        } = echo(&thread, built)
        else {
            panic!("expected a reaction");
        };
        assert_eq!(parsed, target);
        assert_eq!(reaction.emoji, "🎉");
        assert!(!remove);
    }

    #[test]
    fn removing_a_reaction_round_trips_as_a_removal() {
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));

        let built = reaction(&thread, &target(), "🎉".into(), true, 900);

        let Fragment::Reaction { remove, .. } = echo(&thread, built) else {
            panic!("expected a reaction");
        };
        assert!(remove);
    }

    #[test]
    fn a_delete_round_trips_to_its_target() {
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));

        let built = delete(&thread, 500, 900);

        let Fragment::Delete { target } = echo(&thread, built) else {
            panic!("expected a delete");
        };
        assert_eq!(target.timestamp, 500);
    }

    #[test]
    fn an_edit_round_trips_carrying_the_new_body_and_the_old_identity() {
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));

        let built = edit(&thread, 500, "fixed".into(), 900);

        let Fragment::Edit { target, message } = echo(&thread, built) else {
            panic!("expected an edit");
        };
        assert_eq!(target.timestamp, 500);
        assert_eq!(message.text(), Some("fixed"));
    }

    #[test]
    fn a_reply_round_trips_with_the_quoted_snapshot() {
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));
        let target = target();
        let built = replying_to(
            text(&thread, "agreed".into(), 900),
            quote(&target, "the original", &[]),
        );

        let Fragment::Message(message) = echo(&thread, built) else {
            panic!("expected a message");
        };
        assert_eq!(message.text(), Some("agreed"));
        let quoted = message.quote.expect("a quote");
        assert_eq!(quoted.id, target);
        assert_eq!(quoted.body, "the original");
    }

    /// The body offsets go out as UTF-16 and come back as bytes, so a quote of
    /// text with an emoji before a styled range must survive the round trip.
    #[test]
    fn a_quoted_body_keeps_its_ranges_across_the_utf16_boundary() {
        use crate::data::message::range::Style;

        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));
        let body = "🎉 bold here";
        let bold = Range {
            start: body.find("bold").unwrap(),
            len: 4,
            style: Style::Bold,
        };
        let built = replying_to(
            text(&thread, "yes".into(), 900),
            quote(&target(), body, &[bold]),
        );

        let Fragment::Message(message) = echo(&thread, built) else {
            panic!("expected a message");
        };
        let quoted = message.quote.expect("a quote");
        assert_eq!(quoted.ranges, [bold]);
        assert_eq!(&quoted.body[bold.start..bold.end()], "bold");
    }

    #[test]
    fn a_read_receipt_carries_every_timestamp_at_once() {
        let receipt = receipt(receipt_message::Type::Read, vec![1, 2, 3]);

        assert_eq!(receipt.r#type(), receipt_message::Type::Read);
        assert_eq!(receipt.timestamp, [1, 2, 3]);
    }

    #[test]
    fn a_read_sync_names_the_sender_both_ways() {
        let sender = Uuid::new_v4();

        let sync = read_sync(&[(sender, 500)]);

        let read = &sync.read[0];
        assert_eq!(read.timestamp(), 500);
        assert_eq!(read.sender_aci(), sender.to_string());
        assert_eq!(read.sender_aci_binary(), sender.as_bytes());
    }

    #[test]
    fn a_contact_typing_indicator_carries_no_group_id() {
        let thread = Thread::Contact(ContactId::Aci(Uuid::new_v4()));

        assert!(typing(&thread, true, 1).group_id.is_none());
    }

    /// The wire wants the group *identifier* derived from the master key, not the
    /// master key -- sending the key would leak it and would not be recognised.
    #[test]
    fn a_group_typing_indicator_carries_a_derived_identifier() {
        let master_key = [7u8; 32];

        let message = typing(&Thread::Group(master_key), true, 1);

        let id = message.group_id.expect("a group id");
        assert_eq!(id.len(), 32);
        assert_ne!(id, master_key.to_vec());
    }

    #[test]
    fn typing_start_and_stop_differ() {
        let thread = Thread::Group([7u8; 32]);

        assert_eq!(
            typing(&thread, true, 1).action(),
            typing_message::Action::Started
        );
        assert_eq!(
            typing(&thread, false, 1).action(),
            typing_message::Action::Stopped
        );
    }

    /// A group typing indicator must be identifiable by the recipient, which
    /// means the same master key always derives the same id.
    #[test]
    fn the_derived_group_id_is_stable() {
        let a = typing(&Thread::Group([9u8; 32]), true, 1).group_id;
        let b = typing(&Thread::Group([9u8; 32]), true, 2).group_id;

        assert_eq!(a, b);
        assert_ne!(a, typing(&Thread::Group([8u8; 32]), true, 1).group_id);
    }

    /// A group message does carry its master key, so a group echo derives correctly.
    #[test]
    fn a_group_echo_lands_in_the_group_thread() {
        let aci = Uuid::new_v4();
        let thread = Thread::Group([4u8; 32]);
        let message = text(&thread, "team".into(), 77);

        let content = envelope(aci, DeviceId::new(1).unwrap(), &thread, message, 77);
        let (derived, projected) = from_content(&content).unwrap();

        assert_eq!(derived, thread);
        assert_eq!(projected.text(), Some("team"));
    }
}
