use chrono::{DateTime, Local};
use iced::Fill;
use iced::widget::text::Span;
use iced::widget::{column, rich_text, scrollable};

use super::Element;
use crate::data::{History, Message, State, Status};
use crate::theme;

pub fn view<'a, M: 'a>(history: Option<&'a History>, state: &'a State) -> Element<'a, M> {
    let messages = history.map(History::messages).unwrap_or_default();
    let rows = messages.iter().map(|message| line(message, state));

    scrollable(column(rows).spacing(5).padding([10, 12]).width(Fill))
        .anchor_bottom()
        .height(Fill)
        .into()
}

fn line<'a, M: 'a>(message: &'a Message, state: &'a State) -> Element<'a, M> {
    let colors = theme::colors();
    let sender = message.sender();
    let sender_color = if sender == state.aci {
        colors.text
    } else {
        theme::accent(sender.as_bytes())
    };

    let body: Span<'a> = match message.text() {
        Some(text) => Span::new(text),
        None => Span::new(message.summary()).color(colors.dim),
    };

    let mut spans = vec![
        Span::new(format_time(message.timestamp())).color(colors.muted),
        Span::new(" "),
        Span::new(state.sender_name(sender))
            .color(sender_color)
            .font(theme::FONT_BOLD),
        Span::new(": ").color(colors.muted),
        body,
    ];

    if message.edited.is_some() {
        spans.push(Span::new(" (edited)").color(colors.muted).size(11));
    }
    if let Some(status) = message.status {
        spans.push(
            Span::new(format!("  {}", status_label(status)))
                .color(if status == Status::Failed {
                    colors.danger
                } else {
                    colors.muted
                })
                .size(11),
        );
    }

    rich_text(spans).size(13).into()
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Sending => "sending…",
        Status::Failed => "failed to send",
        Status::Sent => "sent",
        Status::Delivered => "delivered",
        Status::Read => "read",
        Status::Viewed => "viewed",
    }
}

fn format_time(timestamp: u64) -> String {
    let Some(time) = DateTime::from_timestamp_millis(timestamp as i64) else {
        return String::new();
    };
    let time = time.with_timezone(&Local);
    if time.date_naive() == Local::now().date_naive() {
        time.format("%H:%M").to_string()
    } else {
        time.format("%b %d %H:%M").to_string()
    }
}
