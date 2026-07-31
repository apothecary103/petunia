//! What right-clicking a message offers.
//!
//! The same verbs the hover bar has, plus the ones that do not earn a button:
//! there is room in a menu for "save as" and "open with", and no room for them
//! over every message.

use gpui::Window;
use gpui_component::IconName;

use super::Item;
use petunia_data::attachment::{Blob, Kind};
use petunia_data::Message;
use crate::ui::message::act::{Act, Dispatch};

/// The menu for one message. `own` decides whether editing and deleting are
/// offered at all: Signal permits neither on someone else's.
pub fn items(message: &Message, own: bool, act: &Dispatch) -> Vec<Item> {
    let id = message.id;
    let mut items = Vec::new();

    if message.is_addressable() {
        items.push(entry(act, "Reply", IconName::Undo, Act::Reply(id)));
        items.push(entry(act, "Forward…", IconName::Redo, Act::Forward(id)));
    }
    if message.text().is_some_and(|text| !text.is_empty()) {
        items.push(entry(act, "Copy text", IconName::Copy, Act::Copy(id)));
    }
    if own && message.is_addressable() && message.text().is_some() {
        items.push(entry(act, "Edit", IconName::Replace, Act::Edit(id)));
    }

    let media = attachments(message, act);
    if !media.is_empty() {
        items.push(Item::Separator);
        items.extend(media);
    }

    items.push(Item::Separator);
    items.push(entry(act, "Message details", IconName::Info, Act::Raw(id)));

    if own && message.is_addressable() {
        items.push(Item::Separator);
        items.push(entry(act, "Delete", IconName::Delete, Act::Delete(id)).danger());
    }

    items
}

/// What can be done with whatever the message is carrying. Only for what is
/// already on disk: "save" for a file that has not been downloaded would be a
/// control that cannot do what it says.
fn attachments(message: &Message, act: &Dispatch) -> Vec<Item> {
    let mut items = Vec::new();

    for attached in &message.attachments {
        let Blob::Cached(path) = &attached.blob else {
            continue;
        };
        if matches!(attached.kind, Kind::Image { .. } | Kind::Video { .. }) {
            items.push(entry(
                act,
                "View full size",
                IconName::Maximize,
                Act::View(path.clone()),
            ));
        }
        items.push(entry(
            act,
            "Save as…",
            IconName::ArrowDown,
            Act::Save(path.clone()),
        ));
        items.push(entry(
            act,
            "Open with…",
            IconName::ExternalLink,
            Act::Open(path.clone()),
        ));
    }

    items
}

/// The menu for a person: the same one the details panel opens, plus a way in.
pub fn person(who: uuid::Uuid, act: &Dispatch) -> Vec<Item> {
    vec![entry(
        act,
        "Show profile",
        IconName::User,
        Act::Inspect(who),
    )]
}

fn entry(act: &Dispatch, label: &'static str, icon: IconName, what: Act) -> Item {
    let act = act.clone();
    Item::new(label, move |window: &mut Window, cx: &mut gpui::App| {
        act(what.clone(), window, cx)
    })
    .icon(icon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    use petunia_data::message::Content;
    use petunia_data::MessageId;
    use uuid::Uuid;

    fn dispatch() -> Dispatch {
        Rc::new(|_, _, _| {})
    }

    fn labels(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Entry { label, .. } => Some(label.to_string()),
                _ => None,
            })
            .collect()
    }

    fn message(body: &str) -> Message {
        Message::plain(
            MessageId {
                timestamp: 1,
                sender: Uuid::new_v4(),
            },
            body.into(),
        )
    }

    #[test]
    fn your_own_message_can_be_edited_and_deleted() {
        let offered = labels(&items(&message("hi"), true, &dispatch()));

        assert!(offered.contains(&"Edit".to_string()));
        assert!(offered.contains(&"Delete".to_string()));
    }

    /// Signal permits neither on someone else's, so offering them would be two
    /// controls that cannot do what they say.
    #[test]
    fn someone_else_s_message_cannot() {
        let offered = labels(&items(&message("hi"), false, &dispatch()));

        assert!(!offered.contains(&"Edit".to_string()));
        assert!(!offered.contains(&"Delete".to_string()));
        assert!(offered.contains(&"Reply".to_string()));
    }

    #[test]
    fn a_tombstone_can_only_be_copied_from() {
        let mut deleted = message("");
        deleted.content = Content::Deleted;

        let offered = labels(&items(&deleted, true, &dispatch()));

        assert!(!offered.contains(&"Reply".to_string()));
        assert!(!offered.contains(&"Forward…".to_string()));
        assert!(!offered.contains(&"Delete".to_string()));
    }

    /// Forwarding is not editing, so it is offered on anyone's message.
    #[test]
    fn anything_still_standing_can_be_forwarded() {
        for own in [true, false] {
            assert!(
                labels(&items(&message("hi"), own, &dispatch())).contains(&"Forward…".to_string())
            );
        }
    }

    /// What the wire said is always askable, including about a tombstone -- that
    /// is exactly when you want to know.
    #[test]
    fn every_message_offers_its_details() {
        let mut deleted = message("");
        deleted.content = Content::Deleted;

        assert!(
            labels(&items(&deleted, false, &dispatch())).contains(&"Message details".to_string())
        );
    }

    #[test]
    fn a_message_with_no_text_offers_no_copy() {
        let mut sticker = message("");
        sticker.content = Content::Sticker(Box::new(petunia_data::message::Sticker {
            pack_id: Vec::new(),
            pack_key: None,
            sticker_id: 0,
            emoji: None,
            image: None,
        }));

        assert!(!labels(&items(&sticker, true, &dispatch())).contains(&"Copy text".to_string()));
    }

    /// Saving something that has not been downloaded cannot do what it says.
    #[test]
    fn an_undownloaded_attachment_offers_nothing() {
        let mut carrying = message("look");
        carrying.attachments.push(petunia_data::attachment::Attachment {
            id: petunia_data::attachment::Id::from_hex("beef"),
            kind: Kind::Image { size: None },
            content_type: "image/png".into(),
            file_name: None,
            size: 1,
            caption: None,
            blob: Blob::Missing,
        });

        let offered = labels(&items(&carrying, true, &dispatch()));

        assert!(!offered.contains(&"Save as…".to_string()));
    }

    #[test]
    fn a_downloaded_picture_can_be_opened_and_saved() {
        let mut carrying = message("look");
        carrying.attachments.push(petunia_data::attachment::Attachment {
            id: petunia_data::attachment::Id::from_hex("beef"),
            kind: Kind::Image { size: None },
            content_type: "image/png".into(),
            file_name: None,
            size: 1,
            caption: None,
            blob: Blob::Cached("/tmp/x.png".into()),
        });

        let offered = labels(&items(&carrying, true, &dispatch()));

        assert!(offered.contains(&"View full size".to_string()));
        assert!(offered.contains(&"Save as…".to_string()));
        assert!(offered.contains(&"Open with…".to_string()));
    }
}
