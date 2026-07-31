use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, text};
use iced::{Center, Color, Fill, Shrink};
use uuid::Uuid;

use super::{Element, avatar};
use crate::data::{self, Contact, ContactId, Group, Thread, contact_name};
use crate::theme;

pub fn view<'a>(
    contacts: &'a [Contact],
    groups: &'a [Group],
    avatars: &'a HashMap<Thread, image::Handle>,
    previews: &'a HashMap<Thread, data::Message>,
    unread: &'a HashMap<Thread, u32>,
    aci: Uuid,
) -> Element<'a, Thread> {
    let mut items = column![].spacing(2).padding([8, 8]);

    for contact in contacts {
        let label = if contact.name.is_empty() {
            short_uuid(contact)
        } else {
            contact.name.clone()
        };
        let thread = Thread::Contact(ContactId::Aci(contact.uuid));
        items = items.push(entry(
            label,
            theme::accent(contact.uuid.as_bytes()),
            avatars.get(&thread),
            preview_text(previews.get(&thread), &thread, aci, contacts),
            unread.get(&thread).copied().unwrap_or(0),
            thread,
        ));
    }

    for group in groups {
        let thread = Thread::Group(group.master_key);
        items = items.push(entry(
            group.title.clone(),
            theme::accent(&group.master_key),
            avatars.get(&thread),
            preview_text(previews.get(&thread), &thread, aci, contacts),
            unread.get(&thread).copied().unwrap_or(0),
            thread,
        ));
    }

    if contacts.is_empty() && groups.is_empty() {
        items = items.push(
            text("Waiting for contacts to sync…")
                .size(12)
                .style(theme::text_dim),
        );
    }

    container(scrollable(items.width(Fill)).height(Fill))
        .width(260)
        .into()
}

fn entry<'a>(
    label: String,
    accent: Color,
    picture: Option<&image::Handle>,
    preview: String,
    unread: u32,
    thread: Thread,
) -> Element<'a, Thread> {
    let mut item = row![
        avatar::view(&label, accent, 26.0, picture),
        column![
            text(truncate(&label, 22)).size(13).height(Shrink),
            text(truncate(&preview, 26))
                .size(11)
                .style(theme::text_dim)
                .height(Shrink),
        ]
        .spacing(1)
        .width(Fill),
    ]
    .spacing(8)
    .align_y(Center);

    if unread > 0 {
        item = item.push(
            container(text(unread.to_string()).size(10).font(theme::FONT_BOLD))
                .padding([2, 5])
                .style(theme::unread_badge),
        );
    }

    button(item)
        .on_press(thread)
        .width(Fill)
        .height(44)
        .padding([4, 6])
        .style(theme::sidebar_entry)
        .into()
}

fn preview_text(
    message: Option<&data::Message>,
    thread: &Thread,
    aci: Uuid,
    contacts: &[Contact],
) -> String {
    let Some(message) = message else {
        return String::new();
    };
    let body = message.summary();
    if message.sender() == aci {
        format!("You: {body}")
    } else if let Thread::Group(_) = thread {
        let name = contact_name(contacts, message.sender()).unwrap_or("?");
        format!("{name}: {body}")
    } else {
        body
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let cut: String = value.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}

fn short_uuid(contact: &Contact) -> String {
    contact.uuid.to_string()[..8].to_string()
}
