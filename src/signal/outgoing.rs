use presage::libsignal_service::content::{Content, DataMessage, GroupContextV2, Metadata};
use presage::libsignal_service::protocol::{DeviceId, ServiceId};
use uuid::Uuid;

use crate::data::Thread;

/// The single builder for anything petunia sends. Both the wire message and the
/// row saved for the optimistic echo come from one value, so the two cannot
/// drift as attachments, reactions and edits are added.
pub fn text(thread: &Thread, body: String, timestamp: u64) -> DataMessage {
    DataMessage {
        body: Some(body),
        timestamp: Some(timestamp),
        group_v2: group_context(thread),
        ..Default::default()
    }
}

/// Wraps an outgoing message as presage would have stored it had it come off
/// the wire, so the send shows up before the server answers.
pub fn envelope(
    aci: Uuid,
    device: DeviceId,
    thread: &Thread,
    message: DataMessage,
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
    use crate::data::{ContactId, from_content};

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
