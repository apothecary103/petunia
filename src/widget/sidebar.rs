use iced::widget::{button, column, container, row, rule, scrollable, text};
use iced::{Center, Color, Fill, Shrink};

use super::{Element, avatar};
use crate::config::Sidebar;
use crate::data::index::Entry;
use crate::data::{State, Thread};
use crate::theme;

pub fn view<'a>(
    state: &'a State,
    config: &Sidebar,
    selected: Option<Thread>,
) -> Element<'a, Thread> {
    let mut items = column![].spacing(1).padding([8, 8]);
    let mut listed = 0;

    for entry in state.index.conversations() {
        let active = selected.as_ref() == Some(&entry.thread);
        items = items.push(thread_entry(entry, state, config, active));
        listed += 1;
    }

    // One row -- Note to Self -- means nothing has synced yet, which is worth
    // saying rather than leaving the reader with an empty column.
    if listed <= 1 {
        items = items.push(
            text(if state.index.is_empty() {
                "Waiting for contacts to sync…"
            } else {
                "No conversations yet. Press cmd+k to find someone."
            })
            .size(12)
            .style(theme::text_dim),
        );
    }

    container(
        column![
            scrollable(items.width(Fill)).height(Fill),
            rule::horizontal(1).style(theme::separator),
            own(state),
        ]
        .height(Fill),
    )
    // Clamped so a hand-edited config cannot make the sidebar unusable.
    .width(config.width.clamp(180.0, 480.0))
    .into()
}

/// Who you are signed in as, pinned to the foot of the sidebar: avatar, name, and
/// whether the message stream is actually up. The connection line is the only
/// place the app says so, and "why is nothing arriving" is the question it
/// answers.
fn own<'a>(state: &'a State) -> Element<'a, Thread> {
    let name = state.own_name();
    let thread = Thread::Contact(crate::data::ContactId::Aci(state.aci));

    container(
        row![
            avatar::view(&name, accent(&thread), 30.0, state.avatar(&thread)),
            column![
                text(name.clone())
                    .size(13)
                    .font(theme::FONT_BOLD)
                    .height(Shrink),
                text(state.connection.label())
                    .size(11)
                    .style(theme::text_dim)
                    .height(Shrink),
            ]
            .spacing(1)
            .width(Fill),
        ]
        .spacing(9)
        .align_y(Center),
    )
    .padding([8, 10])
    .into()
}

fn thread_entry<'a>(
    entry: &'a Entry,
    state: &'a State,
    config: &Sidebar,
    active: bool,
) -> Element<'a, Thread> {
    let colors = theme::colors();
    let unread = entry.unread > 0;

    let mut lines = column![
        text(&entry.name)
            .size(14)
            .font(if unread {
                theme::FONT_BOLD
            } else {
                iced::Font::MONOSPACE
            })
            .height(Shrink)
            .width(Fill),
    ]
    .spacing(1);

    if config.show_preview
        && let Some(body) = preview(entry, state)
    {
        lines = lines.push(text(body).size(12).style(theme::text_dim).height(Shrink));
    }

    let mut item = row![
        avatar::view(
            &entry.name,
            accent(&entry.thread),
            32.0,
            state.avatar(&entry.thread),
        ),
        lines.width(Fill),
    ]
    .spacing(9)
    .align_y(Center);

    // A muted thread shows a dimmed count rather than a coloured badge, which is
    // what Signal does: still countable, no longer shouting.
    if unread {
        let muted = entry.flags.muted_until.is_some();
        let label = text(entry.unread.to_string()).size(11).font(theme::FONT_BOLD);

        item = item.push(if muted {
            container(label.color(colors.muted)).padding([2, 5]).into()
        } else {
            let badge: Element<'_, Thread> = container(label)
                .padding([2, 5])
                .style(theme::unread_badge)
                .into();
            badge
        });
    }

    button(item)
        .on_press(entry.thread.clone())
        .width(Fill)
        .padding([6, 8])
        .style(if active {
            theme::sidebar_selected
        } else {
            theme::sidebar_entry
        })
        .into()
}

/// `None` when there is nothing worth a second line, so the row collapses rather
/// than reserving empty space.
fn preview(entry: &Entry, state: &State) -> Option<String> {
    let message = entry.preview.as_ref()?;
    let body = message.summary();
    if body.is_empty() {
        return None;
    }

    Some(if message.sender() == state.aci {
        format!("You: {body}")
    } else if matches!(entry.thread, Thread::Group(_)) {
        format!("{}: {body}", state.sender_name(message.sender()))
    } else {
        body
    })
}

fn accent(thread: &Thread) -> Color {
    match thread {
        Thread::Contact(contact) => theme::accent(contact.uuid().as_bytes()),
        Thread::Group(master_key) => theme::accent(master_key),
    }
}
