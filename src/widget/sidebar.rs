use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Center, Color, Fill, Shrink};

use super::{Element, avatar};
use crate::data::index::Entry;
use crate::data::{State, Thread};
use crate::theme;

pub fn view(state: &State) -> Element<'_, Thread> {
    let mut items = column![].spacing(2).padding([8, 8]);

    if state.index.is_empty() {
        items = items.push(
            text("Waiting for contacts to sync…")
                .size(12)
                .style(theme::text_dim),
        );
    }

    for entry in state.index.entries() {
        items = items.push(thread_entry(entry, state));
    }

    container(scrollable(items.width(Fill)).height(Fill))
        .width(260)
        .into()
}

fn thread_entry<'a>(entry: &'a Entry, state: &'a State) -> Element<'a, Thread> {
    let mut item = row![
        avatar::view(
            &entry.name,
            accent(&entry.thread),
            26.0,
            state.avatar(&entry.thread),
        ),
        column![
            text(truncate(&entry.name, 22)).size(13).height(Shrink),
            text(truncate(&preview(entry, state), 26))
                .size(11)
                .style(theme::text_dim)
                .height(Shrink),
        ]
        .spacing(1)
        .width(Fill),
    ]
    .spacing(8)
    .align_y(Center);

    if entry.unread > 0 {
        item = item.push(
            container(text(entry.unread.to_string()).size(10).font(theme::FONT_BOLD))
                .padding([2, 5])
                .style(theme::unread_badge),
        );
    }

    button(item)
        .on_press(entry.thread.clone())
        .width(Fill)
        .height(44)
        .padding([4, 6])
        .style(theme::sidebar_entry)
        .into()
}

fn preview(entry: &Entry, state: &State) -> String {
    let Some(message) = &entry.preview else {
        return String::new();
    };
    let body = message.summary();
    if message.sender() == state.aci {
        format!("You: {body}")
    } else if let Thread::Group(_) = entry.thread {
        format!("{}: {body}", state.sender_name(message.sender()))
    } else {
        body
    }
}

fn accent(thread: &Thread) -> Color {
    match thread {
        Thread::Contact(contact) => theme::accent(contact.uuid().as_bytes()),
        Thread::Group(master_key) => theme::accent(master_key),
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
