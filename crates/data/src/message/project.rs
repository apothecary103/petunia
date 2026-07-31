use std::collections::BTreeMap;

use presage::libsignal_service::content::{Content as Envelope, ContentBody};
use presage::libsignal_service::proto::sync_message::Sent;
use presage::libsignal_service::proto::{
    AttachmentPointer, DataMessage, EditMessage, SyncMessage, data_message, receipt_message,
};
use presage::store::ContentExt;
use uuid::Uuid;

use super::range;
use super::{Content, LinkPreview, Message, MessageId, Quote, Reaction, Status, Sticker, Update};
use crate::{Thread, attachment};

#[derive(Debug, Clone)]
pub enum Fragment {
    Message(Message),
    Edit { target: MessageId, message: Message },
    Reaction { target: MessageId, reaction: Reaction, remove: bool },
    Delete { target: MessageId },
    Ignored,
}

pub fn classify(envelope: &Envelope) -> Option<(Thread, Fragment)> {
    let thread = presage::store::Thread::try_from(envelope).ok()?;
    let sender = envelope.metadata.sender.raw_uuid();
    let sent_at = envelope.metadata.timestamp.timestamp_millis() as u64;

    let fragment = match body(&envelope.body)? {
        Body::Data(data) => data_fragment(data, sender, envelope.timestamp()),
        Body::Edit(edit) => match &edit.data_message {
            Some(data) => Fragment::Edit {
                target: MessageId {
                    timestamp: edit.target_sent_timestamp(),
                    sender,
                },
                message: message(data, MessageId { timestamp: sent_at, sender }),
            },
            None => Fragment::Ignored,
        },
    };

    Some(((&thread).into(), fragment))
}

pub fn project(rows: impl IntoIterator<Item = Envelope>) -> Vec<Message> {
    let mut fragments: Vec<(u64, Fragment)> = rows
        .into_iter()
        .filter_map(|envelope| {
            let order = envelope.metadata.timestamp.timestamp_millis() as u64;
            classify(&envelope).map(|(_, fragment)| (order, fragment))
        })
        .collect();
    fragments.sort_by_key(|(order, _)| *order);

    let mut messages: BTreeMap<MessageId, Message> = BTreeMap::new();
    let mut modifiers = Vec::new();

    for (order, fragment) in fragments {
        match fragment {
            Fragment::Message(message) => {
                messages.insert(message.id, message);
            }
            Fragment::Ignored => {}
            other => modifiers.push((order, other)),
        }
    }

    for (order, fragment) in modifiers {
        match fragment {
            Fragment::Edit { target, message } => {
                if let Some(existing) = messages.get_mut(&target) {
                    apply_edit(existing, message, order);
                }
            }
            Fragment::Reaction {
                target,
                reaction,
                remove,
            } => {
                if let Some(existing) = messages.get_mut(&target) {
                    apply_reaction(existing, reaction, remove);
                }
            }
            Fragment::Delete { target } => {
                if let Some(existing) = messages.get_mut(&target) {
                    apply_delete(existing);
                }
            }
            Fragment::Message(_) | Fragment::Ignored => {}
        }
    }

    messages.into_values().collect()
}

pub fn apply_edit(message: &mut Message, edit: Message, edited_at: u64) {
    message.content = edit.content;
    message.attachments = edit.attachments;
    message.preview = edit.preview;
    message.edited = Some(edited_at);
}

pub fn apply_reaction(message: &mut Message, reaction: Reaction, remove: bool) {
    message
        .reactions
        .retain(|existing| existing.author != reaction.author);
    if !remove {
        message.reactions.push(reaction);
    }
}

pub fn apply_delete(message: &mut Message) {
    message.content = Content::Deleted;
    message.attachments.clear();
    message.quote = None;
    message.preview = None;
    message.reactions.clear();
}

/// Every downloadable pointer a stored row carries, paired with the id the UI
/// knows it by. The pointers themselves are never handed to the UI -- they are
/// bulky, protocol-shaped and expire -- so the worker re-reads them from the
/// store when a download is asked for.
pub fn pointers(envelope: &Envelope) -> Vec<(attachment::Id, AttachmentPointer)> {
    let Some(body) = body(&envelope.body) else {
        return Vec::new();
    };
    let data = match body {
        Body::Data(data) => data,
        Body::Edit(edit) => match &edit.data_message {
            Some(data) => data,
            None => return Vec::new(),
        },
    };

    let quote = data
        .quote
        .iter()
        .flat_map(|quote| quote.attachments.iter())
        .filter_map(|attached| attached.thumbnail.as_ref());
    let preview = data.preview.iter().filter_map(|preview| preview.image.as_ref());
    let sticker = data.sticker.iter().filter_map(|sticker| sticker.data.as_ref());

    data.attachments
        .iter()
        .chain(quote)
        .chain(preview)
        .chain(sticker)
        .filter_map(|pointer| {
            let id = attachment::from_pointer(pointer)?.id;
            Some((id, pointer.clone()))
        })
        .collect()
}

/// The plain-message half of `classify`, which is all the tests that predate
/// fragments care about. Nothing in the app uses it: the receive path needs the
/// reactions, edits and deletes that `classify` keeps.
#[cfg(test)]
fn from_content(envelope: &Envelope) -> Option<(Thread, Message)> {
    match classify(envelope)? {
        (thread, Fragment::Message(message)) => Some((thread, message)),
        _ => None,
    }
}

pub fn receipt_from_content(envelope: &Envelope) -> Option<(Vec<u64>, Status)> {
    let ContentBody::ReceiptMessage(receipt) = &envelope.body else {
        return None;
    };
    let status = match receipt.r#type() {
        receipt_message::Type::Delivery => Status::Delivered,
        receipt_message::Type::Read => Status::Read,
        receipt_message::Type::Viewed => Status::Viewed,
    };
    Some((receipt.timestamp.clone(), status))
}

enum Body<'a> {
    Data(&'a DataMessage),
    Edit(&'a EditMessage),
}

fn body(content: &ContentBody) -> Option<Body<'_>> {
    match content {
        ContentBody::DataMessage(data) => Some(Body::Data(data)),
        ContentBody::EditMessage(edit) => Some(Body::Edit(edit)),
        ContentBody::SynchronizeMessage(SyncMessage { sent: Some(sent), .. }) => match sent {
            Sent {
                edit_message: Some(edit),
                ..
            } => Some(Body::Edit(edit)),
            Sent {
                message: Some(data),
                ..
            } => Some(Body::Data(data)),
            _ => None,
        },
        _ => None,
    }
}

fn data_fragment(data: &DataMessage, sender: Uuid, timestamp: u64) -> Fragment {
    if let Some(reaction) = &data.reaction {
        let Some(target) = target_id(
            reaction.target_sent_timestamp,
            &reaction.target_author_aci,
            &reaction.target_author_aci_binary,
        ) else {
            return Fragment::Ignored;
        };
        return Fragment::Reaction {
            target,
            reaction: Reaction {
                author: sender,
                emoji: reaction.emoji().to_string(),
                timestamp,
            },
            remove: reaction.remove(),
        };
    }

    if let Some(delete) = &data.delete {
        return match delete.target_sent_timestamp {
            Some(target) => Fragment::Delete {
                target: MessageId {
                    timestamp: target,
                    sender,
                },
            },
            None => Fragment::Ignored,
        };
    }

    let id = MessageId { timestamp, sender };
    let message = message(data, id);

    match &message.content {
        Content::Text { body, .. }
            if body.is_empty() && message.attachments.is_empty() && message.quote.is_none() =>
        {
            Fragment::Ignored
        }
        _ => Fragment::Message(message),
    }
}

fn message(data: &DataMessage, id: MessageId) -> Message {
    let mut message = Message::new(id, content(data));

    message.attachments = data
        .attachments
        .iter()
        .filter_map(attachment::from_pointer)
        .collect();
    message.quote = data.quote.as_ref().and_then(quote).map(Box::new);
    message.preview = data.preview.first().and_then(preview).map(Box::new);
    message.expires_in = data.expire_timer;
    message.view_once = data.is_view_once();

    message
}

fn content(data: &DataMessage) -> Content {
    if is_timer_update(data) {
        return Content::Update(Update::ExpireTimer {
            seconds: data.expire_timer(),
        });
    }

    if let Some(sticker) = &data.sticker {
        return Content::Sticker(Box::new(Sticker {
            pack_id: sticker.pack_id().to_vec(),
            // Carried so a received sticker can offer its own pack: installing
            // one needs the key, and it only ever travels with the sticker.
            pack_key: sticker.pack_key.clone(),
            sticker_id: sticker.sticker_id(),
            emoji: sticker.emoji.clone(),
            image: sticker.data.as_ref().and_then(attachment::from_pointer),
        }));
    }

    let body = data.body().to_string();
    Content::Text {
        ranges: range::from_proto(&body, &data.body_ranges),
        body,
    }
}

fn quote(quote: &data_message::Quote) -> Option<Quote> {
    let id = target_id(quote.id, &quote.author_aci, &quote.author_aci_binary)?;
    let body = quote.text().to_string();

    let attached = quote.attachments.first();

    Some(Quote {
        id,
        ranges: range::from_proto(&body, &quote.body_ranges),
        body,
        // The file's own name for a file, and what it is for anything with a
        // shape of its own -- "report.pdf" says more than "File", and "Photo"
        // says more than a camera's serial number.
        media: attached.map(|attached| {
            let kind = attachment::classify(attached.content_type(), None);
            match (&kind, attached.file_name.as_deref()) {
                (attachment::Kind::File, Some(name)) if !name.is_empty() => name.to_owned(),
                _ => kind.label().to_owned(),
            }
        }),
        thumbnail: attached
            .and_then(|attached| attached.thumbnail.as_ref())
            .and_then(attachment::from_pointer),
    })
}

fn preview(preview: &presage::libsignal_service::proto::Preview) -> Option<LinkPreview> {
    Some(LinkPreview {
        url: preview.url.clone()?,
        title: preview.title.clone(),
        description: preview.description.clone(),
        image: preview.image.as_ref().and_then(attachment::from_pointer),
    })
}

fn is_timer_update(data: &DataMessage) -> bool {
    data.flags() & data_message::Flags::ExpirationTimerUpdate as u32 != 0
}

fn target_id(
    timestamp: Option<u64>,
    aci: &Option<String>,
    binary: &Option<Vec<u8>>,
) -> Option<MessageId> {
    Some(MessageId {
        timestamp: timestamp?,
        sender: uuid(aci, binary)?,
    })
}

fn uuid(aci: &Option<String>, binary: &Option<Vec<u8>>) -> Option<Uuid> {
    if let Some(bytes) = binary.as_deref()
        && let Ok(bytes) = <[u8; 16]>::try_from(bytes)
    {
        return Some(Uuid::from_bytes(bytes));
    }
    aci.as_deref()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use presage::libsignal_service::content::Metadata;
    use presage::libsignal_service::proto::{
        AttachmentPointer, GroupContextV2, ReceiptMessage, receipt_message,
    };
    use presage::libsignal_service::protocol::ServiceId;
    use presage::libsignal_service::push_service::DEFAULT_DEVICE_ID;
    use presage::store::ContentsStore;
    use presage_store_sqlite::{OnNewIdentity, SqliteStore};

    use super::*;
    use crate::ContactId;
    use crate::message::range::Style;

    fn metadata(sender: Uuid, timestamp: u64) -> Metadata {
        let time = chrono::DateTime::from_timestamp_millis(timestamp as i64).unwrap();
        Metadata {
            sender: ServiceId::Aci(sender.into()),
            destination: ServiceId::Aci(sender.into()),
            sender_device: *DEFAULT_DEVICE_ID,
            timestamp: time,
            server_timestamp: time,
            needs_receipt: false,
            unidentified_sender: false,
            was_plaintext: false,
            server_guid: None,
        }
    }

    fn text_message(sender: Uuid, timestamp: u64, body: &str) -> Envelope {
        Envelope::from_body(
            DataMessage {
                body: Some(body.into()),
                timestamp: Some(timestamp),
                ..Default::default()
            },
            metadata(sender, timestamp),
        )
    }

    fn reaction(
        author: Uuid,
        at: u64,
        target: MessageId,
        emoji: &str,
        remove: bool,
    ) -> Envelope {
        Envelope::from_body(
            DataMessage {
                timestamp: Some(at),
                reaction: Some(data_message::Reaction {
                    emoji: Some(emoji.into()),
                    remove: Some(remove),
                    target_sent_timestamp: Some(target.timestamp),
                    target_author_aci_binary: Some(target.sender.as_bytes().to_vec()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            metadata(author, at),
        )
    }

    fn edit(sender: Uuid, at: u64, target: u64, body: &str) -> Envelope {
        Envelope::from_body(
            EditMessage {
                target_sent_timestamp: Some(target),
                data_message: Some(DataMessage {
                    body: Some(body.into()),
                    timestamp: Some(at),
                    ..Default::default()
                }),
            },
            metadata(sender, at),
        )
    }

    fn delete(sender: Uuid, at: u64, target: u64) -> Envelope {
        Envelope::from_body(
            DataMessage {
                timestamp: Some(at),
                delete: Some(data_message::Delete {
                    target_sent_timestamp: Some(target),
                }),
                ..Default::default()
            },
            metadata(sender, at),
        )
    }

    #[test]
    fn maps_incoming_text_to_contact_thread() {
        let sender = Uuid::new_v4();
        let (thread, message) = from_content(&text_message(sender, 1234, "hi")).unwrap();

        assert_eq!(thread, Thread::Contact(ContactId::Aci(sender)));
        assert_eq!(message.sender(), sender);
        assert_eq!(message.timestamp(), 1234);
        assert_eq!(message.text(), Some("hi"));
    }

    #[test]
    fn maps_group_message_to_group_thread() {
        let master_key = [7u8; 32];
        let content = Envelope::from_body(
            DataMessage {
                body: Some("hello group".into()),
                timestamp: Some(1234),
                group_v2: Some(GroupContextV2 {
                    master_key: Some(master_key.to_vec()),
                    revision: Some(0),
                    ..Default::default()
                }),
                ..Default::default()
            },
            metadata(Uuid::new_v4(), 1234),
        );

        let (thread, message) = from_content(&content).unwrap();
        assert_eq!(thread, Thread::Group(master_key));
        assert_eq!(message.text(), Some("hello group"));
    }

    #[test]
    fn maps_synced_sent_message_to_destination_thread() {
        let own = Uuid::new_v4();
        let destination = Uuid::new_v4();
        let content = Envelope::from_body(
            SyncMessage {
                sent: Some(Sent {
                    destination_service_id: Some(destination.to_string()),
                    timestamp: Some(9999),
                    message: Some(DataMessage {
                        body: Some("from my phone".into()),
                        timestamp: Some(9999),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            metadata(own, 9999),
        );

        let (thread, message) = from_content(&content).unwrap();
        assert_eq!(thread, Thread::Contact(ContactId::Aci(destination)));
        assert_eq!(message.sender(), own);
        assert_eq!(message.timestamp(), 9999);
        assert_eq!(message.text(), Some("from my phone"));
    }

    #[test]
    fn maps_read_receipt_to_statuses() {
        let content = Envelope::from_body(
            ReceiptMessage {
                r#type: Some(receipt_message::Type::Read as i32),
                timestamp: vec![1234, 5678],
            },
            metadata(Uuid::new_v4(), 9999),
        );
        assert_eq!(
            receipt_from_content(&content),
            Some((vec![1234, 5678], Status::Read))
        );
    }

    #[test]
    fn maps_a_viewed_receipt_rather_than_dropping_it() {
        let content = Envelope::from_body(
            ReceiptMessage {
                r#type: Some(receipt_message::Type::Viewed as i32),
                timestamp: vec![1],
            },
            metadata(Uuid::new_v4(), 2),
        );
        assert_eq!(
            receipt_from_content(&content),
            Some((vec![1], Status::Viewed))
        );
    }

    #[test]
    fn ignores_data_message_without_body() {
        let content = Envelope::from_body(DataMessage::default(), metadata(Uuid::new_v4(), 1234));
        assert!(from_content(&content).is_none());
    }

    #[tokio::test]
    async fn maps_messages_round_tripped_through_store() {
        let store = SqliteStore::open(":memory:", OnNewIdentity::Trust)
            .await
            .unwrap();

        let sender = Uuid::new_v4();
        let content = text_message(sender, 1234, "stored");
        let thread = presage::store::Thread::try_from(&content).unwrap();
        store.save_message(&thread, content).await.unwrap();

        let stored: Vec<_> = store
            .messages(&thread, ..)
            .await
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let (mapped_thread, message) = from_content(&stored[0]).unwrap();

        assert_eq!(mapped_thread, Thread::Contact(ContactId::Aci(sender)));
        assert_eq!(message.text(), Some("stored"));
        assert_eq!(message.timestamp(), 1234);
    }

    #[test]
    fn keeps_an_attachment_only_message() {
        let content = Envelope::from_body(
            DataMessage {
                timestamp: Some(5),
                attachments: vec![AttachmentPointer {
                    digest: Some(vec![1]),
                    content_type: Some("image/png".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            metadata(Uuid::new_v4(), 5),
        );

        let (_, message) = from_content(&content).unwrap();
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.summary(), "Photo");
    }

    #[test]
    fn reads_body_ranges_as_byte_offsets() {
        let content = Envelope::from_body(
            DataMessage {
                body: Some("😀 bold".into()),
                timestamp: Some(1),
                body_ranges: vec![presage::libsignal_service::proto::BodyRange {
                    start: Some(3),
                    length: Some(4),
                    associated_value: Some(
                        presage::libsignal_service::proto::body_range::AssociatedValue::Style(
                            presage::libsignal_service::proto::body_range::Style::Bold as i32,
                        ),
                    ),
                }],
                ..Default::default()
            },
            metadata(Uuid::new_v4(), 1),
        );

        let (_, message) = from_content(&content).unwrap();
        let range = message.ranges()[0];
        assert_eq!(range.style, Style::Bold);
        assert_eq!(&message.text().unwrap()[range.start..range.end()], "bold");
    }

    #[test]
    fn classifies_a_sticker() {
        let content = Envelope::from_body(
            DataMessage {
                timestamp: Some(1),
                sticker: Some(data_message::Sticker {
                    pack_id: Some(vec![9]),
                    sticker_id: Some(3),
                    emoji: Some("🎉".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            metadata(Uuid::new_v4(), 1),
        );

        let (_, message) = from_content(&content).unwrap();
        assert!(matches!(message.content, Content::Sticker(_)));
        assert_eq!(message.summary(), "🎉 Sticker");
    }

    #[test]
    fn classifies_an_expire_timer_update() {
        let content = Envelope::from_body(
            DataMessage {
                timestamp: Some(1),
                flags: Some(data_message::Flags::ExpirationTimerUpdate as u32),
                expire_timer: Some(604_800),
                ..Default::default()
            },
            metadata(Uuid::new_v4(), 1),
        );

        let (_, message) = from_content(&content).unwrap();
        assert!(matches!(
            message.content,
            Content::Update(Update::ExpireTimer { seconds: 604_800 })
        ));
    }

    #[test]
    fn folds_an_edit_onto_the_original() {
        let sender = Uuid::new_v4();
        let folded = project([
            text_message(sender, 100, "typo"),
            edit(sender, 200, 100, "fixed"),
        ]);

        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].timestamp(), 100);
        assert_eq!(folded[0].text(), Some("fixed"));
        assert_eq!(folded[0].edited, Some(200));
    }

    #[test]
    fn applies_the_latest_edit_regardless_of_row_order() {
        let sender = Uuid::new_v4();
        let folded = project([
            edit(sender, 300, 100, "third"),
            text_message(sender, 100, "first"),
            edit(sender, 200, 100, "second"),
        ]);

        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].text(), Some("third"));
        assert_eq!(folded[0].edited, Some(300));
    }

    #[test]
    fn folds_a_reaction_onto_its_target() {
        let author = Uuid::new_v4();
        let reactor = Uuid::new_v4();
        let target = MessageId {
            timestamp: 100,
            sender: author,
        };
        let folded = project([
            text_message(author, 100, "nice"),
            reaction(reactor, 200, target, "👍", false),
        ]);

        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].reactions.len(), 1);
        assert_eq!(folded[0].reactions[0].emoji, "👍");
        assert_eq!(folded[0].reactions[0].author, reactor);
    }

    #[test]
    fn replaces_a_reaction_from_the_same_author() {
        let author = Uuid::new_v4();
        let reactor = Uuid::new_v4();
        let target = MessageId {
            timestamp: 100,
            sender: author,
        };
        let folded = project([
            text_message(author, 100, "nice"),
            reaction(reactor, 200, target, "👍", false),
            reaction(reactor, 300, target, "🎉", false),
        ]);

        assert_eq!(folded[0].reactions.len(), 1);
        assert_eq!(folded[0].reactions[0].emoji, "🎉");
    }

    #[test]
    fn removes_a_reaction_when_toggled_off() {
        let author = Uuid::new_v4();
        let reactor = Uuid::new_v4();
        let target = MessageId {
            timestamp: 100,
            sender: author,
        };
        let folded = project([
            text_message(author, 100, "nice"),
            reaction(reactor, 200, target, "👍", false),
            reaction(reactor, 300, target, "👍", true),
        ]);

        assert!(folded[0].reactions.is_empty());
    }

    #[test]
    fn keeps_reactions_from_different_authors() {
        let author = Uuid::new_v4();
        let target = MessageId {
            timestamp: 100,
            sender: author,
        };
        let folded = project([
            text_message(author, 100, "nice"),
            reaction(Uuid::new_v4(), 200, target, "👍", false),
            reaction(Uuid::new_v4(), 300, target, "👍", false),
        ]);

        assert_eq!(folded[0].reactions.len(), 2);
    }

    #[test]
    fn tombstones_a_deleted_message() {
        let sender = Uuid::new_v4();
        let target = MessageId {
            timestamp: 100,
            sender,
        };
        let folded = project([
            text_message(sender, 100, "oops"),
            reaction(Uuid::new_v4(), 150, target, "👍", false),
            delete(sender, 200, 100),
        ]);

        assert_eq!(folded.len(), 1);
        assert!(matches!(folded[0].content, Content::Deleted));
        assert!(folded[0].reactions.is_empty());
        assert_eq!(folded[0].summary(), "This message was deleted");
    }

    #[test]
    fn an_edit_after_a_delete_still_loses_to_the_delete() {
        let sender = Uuid::new_v4();
        let folded = project([
            text_message(sender, 100, "oops"),
            edit(sender, 200, 100, "fixed"),
            delete(sender, 300, 100),
        ]);

        assert!(matches!(folded[0].content, Content::Deleted));
    }

    #[test]
    fn drops_modifiers_whose_target_is_outside_the_page() {
        let sender = Uuid::new_v4();
        let missing = MessageId {
            timestamp: 1,
            sender,
        };
        let folded = project([
            text_message(sender, 100, "here"),
            reaction(Uuid::new_v4(), 200, missing, "👍", false),
            edit(sender, 300, 1, "ghost"),
            delete(sender, 400, 1),
        ]);

        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].text(), Some("here"));
    }

    #[test]
    fn returns_messages_in_ascending_timestamp_order() {
        let sender = Uuid::new_v4();
        let folded = project([
            text_message(sender, 300, "c"),
            text_message(sender, 100, "a"),
            text_message(sender, 200, "b"),
        ]);

        let bodies: Vec<_> = folded.iter().filter_map(|m| m.text()).collect();
        assert_eq!(bodies, ["a", "b", "c"]);
    }

    #[test]
    fn reads_a_quote() {
        let author = Uuid::new_v4();
        let content = Envelope::from_body(
            DataMessage {
                body: Some("agreed".into()),
                timestamp: Some(200),
                quote: Some(data_message::Quote {
                    id: Some(100),
                    author_aci_binary: Some(author.as_bytes().to_vec()),
                    text: Some("original".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            metadata(Uuid::new_v4(), 200),
        );

        let (_, message) = from_content(&content).unwrap();
        let quote = message.quote.unwrap();
        assert_eq!(quote.id.timestamp, 100);
        assert_eq!(quote.id.sender, author);
        assert_eq!(quote.body, "original");
        assert_eq!(quote.media, None);
    }

    /// A picture sent with no caption quotes as no text at all, so the quote has
    /// to carry what it *was* or the bar draws a name over an empty line.
    #[test]
    fn a_quote_of_media_says_what_was_quoted() {
        let quoted = |content_type: &str, file_name: Option<&str>| {
            let content = Envelope::from_body(
                DataMessage {
                    body: Some("nice".into()),
                    timestamp: Some(200),
                    quote: Some(data_message::Quote {
                        id: Some(100),
                        author_aci_binary: Some(Uuid::new_v4().as_bytes().to_vec()),
                        attachments: vec![data_message::quote::QuotedAttachment {
                            content_type: Some(content_type.into()),
                            file_name: file_name.map(str::to_owned),
                            thumbnail: None,
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                metadata(Uuid::new_v4(), 200),
            );
            from_content(&content).unwrap().1.quote.unwrap().media
        };

        assert_eq!(quoted("image/jpeg", None).as_deref(), Some("Photo"));
        assert_eq!(quoted("video/mp4", None).as_deref(), Some("Video"));
        // A file has nothing to look at, so its name is the only useful thing to
        // say about it.
        assert_eq!(
            quoted("application/pdf", Some("report.pdf")).as_deref(),
            Some("report.pdf")
        );
        assert_eq!(quoted("application/pdf", None).as_deref(), Some("File"));
    }
}
