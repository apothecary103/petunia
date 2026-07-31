pub mod content;
pub mod format;
pub mod group;
mod grouped;
mod irc;

use chrono::{Local, NaiveDate};
use iced::widget::{Space, button, center, column, container, row, rule, scrollable, text};
use iced::{Center, Fill, Shrink};
use uuid::Uuid;

use super::Element;
use crate::data::attachment;
use crate::config::messages::{Layout, Spacing, Timestamps};
use crate::data::{History, MessageId, State};
use crate::theme;

#[derive(Debug, Clone)]
pub enum Message {
    LoadOlder,
    Download(u64, attachment::Id),
    OpenAttachment(std::path::PathBuf),
    React(MessageId, String, bool),
    Reply(MessageId),
    Edit(MessageId),
    Delete(MessageId),
    Copy(String),
    Link(Link),
}

/// What a clickable span in a body points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Url(String),
    /// Which spoiler segment to uncover: the message it is in, and the byte
    /// offset of the segment within that body.
    Reveal(MessageId, usize),
}

/// Everything the frames need to render but do not own.
pub struct Context<'a> {
    pub state: &'a State,
    pub messages: &'a crate::config::Messages,
    pub layout: Layout,
    pub image_max_width: f32,
    pub image_max_height: f32,
    /// Spoiler segments the reader has clicked open, as (message, byte offset).
    pub revealed: &'a [(u64, usize)],
}

impl Context<'_> {
    pub fn spacing(&self) -> Spacing {
        self.messages.density.spacing(self.layout)
    }

    pub fn timestamps(&self) -> Timestamps {
        self.messages.timestamps
    }

    fn avatar(&self, sender: Uuid) -> Option<&iced::widget::image::Handle> {
        self.state.avatar_for(sender)
    }
}

pub fn view<'a>(
    history: Option<&'a History>,
    context: Context<'a>,
    id: iced::widget::Id,
) -> Element<'a, Message> {
    let messages = history.map(History::messages).unwrap_or_default();
    let spacing = context.spacing();
    let entries = group::entries(
        messages,
        history.and_then(History::first_unread),
        context.messages.group_within * 1000,
    );

    let mut rows = column![].spacing(spacing.between_runs).width(Fill);

    if let Some(history) = history
        && history.has_more()
    {
        rows = rows.push(older_button(history.is_loading()));
    }

    for entry in &entries {
        rows = rows.push(match entry {
            group::Entry::Day(date) if context.messages.date_separators => {
                day_separator(*date, spacing.small)
            }
            group::Entry::Day(_) => Space::new().into(),
            group::Entry::UnreadMarker => unread_marker(spacing.small),
            group::Entry::Update(message) => update_line(message, &context),
            group::Entry::Run(run) => match context.layout {
                Layout::Grouped => grouped::run(run, &context),
                Layout::Irc => irc::run(run, &context),
            },
        });
    }

    scrollable(rows.padding([spacing.padding_y, spacing.padding_x]))
        .id(id)
        .anchor_bottom()
        .height(Fill)
        .into()
}

fn older_button<'a>(loading: bool) -> Element<'a, Message> {
    let label = if loading {
        "loading older messages…"
    } else {
        "load older messages"
    };
    let mut control = button(text(label).size(12).height(Shrink)).style(theme::pane_control);
    if !loading {
        control = control.on_press(Message::LoadOlder);
    }
    center(control).height(Shrink).into()
}

fn day_separator<'a>(date: NaiveDate, size: f32) -> Element<'a, Message> {
    let today = Local::now().date_naive();
    let label = if date == today {
        "Today".to_string()
    } else if Some(date) == today.pred_opt() {
        "Yesterday".to_string()
    } else if date.format("%Y").to_string() == today.format("%Y").to_string() {
        date.format("%A, %e %B").to_string()
    } else {
        date.format("%e %B %Y").to_string()
    };

    row![
        rule::horizontal(1).style(theme::separator),
        text(label)
            .size(size)
            .color(theme::colors().muted)
            .font(theme::FONT_BOLD)
            .height(Shrink),
        rule::horizontal(1).style(theme::separator),
    ]
    .spacing(8)
    .align_y(Center)
    .padding([4, 0])
    .into()
}

fn unread_marker<'a>(size: f32) -> Element<'a, Message> {
    let colors = theme::colors();
    row![
        container(Space::new().width(Fill).height(1)).style(|_theme| container::Style {
            background: Some(iced::Background::Color(theme::colors().danger)),
            ..container::Style::default()
        }),
        text("new")
            .size(size)
            .color(colors.danger)
            .font(theme::FONT_BOLD)
            .height(Shrink),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

/// System lines are centered and unattributed: nobody said them.
fn update_line<'a>(
    message: &'a crate::data::Message,
    context: &Context<'_>,
) -> Element<'a, Message> {
    center(content::body(content::spans(message, context), context))
        .height(Shrink)
        .padding([2, 0])
        .into()
}
