use chrono::{DateTime, Local};
use iced::Fill;
use iced::widget::text::Span;
use iced::widget::{column, rich_text, scrollable};
use uuid::Uuid;

use super::Element;
use crate::data::{Contact, Message, Status, contact_name};
use crate::theme;

pub fn view<'a, M: 'a>(messages: &'a [Message], aci: Uuid, contacts: &'a [Contact]) -> Element<'a, M> {
    let colors = theme::colors();
    let rows = messages.iter().map(|message| {
        let own = message.sender() == aci;
        let sender_color = if own {
            colors.text
        } else {
            theme::accent(message.sender().as_bytes())
        };
        let body = match message.text() {
            Some(text) => Span::new(text),
            None => Span::new(message.summary()).color(colors.dim),
        };
        let mut spans: Vec<Span<'a>> = vec![
            Span::new(format_time(message.timestamp())).color(colors.muted),
            Span::new(" "),
            Span::new(sender_name(message.sender(), aci, contacts))
                .color(sender_color)
                .font(theme::FONT_BOLD),
            Span::new(": ").color(colors.muted),
            body,
        ];
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
    });

    scrollable(column(rows).spacing(5).padding([10, 12]).width(Fill))
        .anchor_bottom()
        .height(Fill)
        .into()
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

fn sender_name(sender: Uuid, aci: Uuid, contacts: &[Contact]) -> String {
    if sender == aci {
        return "You".into();
    }
    contact_name(contacts, sender)
        .map(str::to_string)
        .unwrap_or_else(|| sender.to_string()[..8].to_string())
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
