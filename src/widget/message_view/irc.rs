use iced::widget::text::Span;
use iced::widget::column;

use super::group::Run;
use super::{Context, Link, Message, content};
use crate::config::messages::Timestamps;
use crate::theme;
use crate::widget::Element;

/// One line per message: time, sender, body, all in a single `rich_text` so a
/// wrapped line hangs under the body rather than under the timestamp.
pub fn run<'a>(run: &Run<'a>, context: &Context<'_>) -> Element<'a, Message> {
    let colors = theme::colors();
    let name = context.state.sender_name(run.sender);
    let color = if run.sender == context.state.aci {
        colors.text
    } else {
        theme::accent(run.sender.as_bytes())
    };

    let spacing = context.spacing();
    let show_time = context.timestamps() != Timestamps::Never;

    let lines = run.messages.iter().map(|message| {
        let mut prefix: Vec<Span<'a, Link>> = Vec::new();
        if show_time {
            prefix.push(
                Span::new(content::clock(message.timestamp()))
                    .color(colors.muted)
                    .size(spacing.small),
            );
            prefix.push(Span::new(" "));
        }
        prefix.push(
            Span::new(format!("{name}: "))
                .color(color)
                .font(theme::FONT_BOLD),
        );

        // A one-line frame gets a one-line reply marker; the block form would
        // cost three lines per reply and bury the reply itself.
        content::one(
            message,
            context,
            content::quote_line(message, context),
            prefix,
            content::spans(message, context),
        )
    });

    column(lines).spacing(spacing.within_run).into()
}
