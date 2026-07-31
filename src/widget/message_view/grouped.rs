use iced::widget::{column, container, row, text, tooltip};
use iced::{Center, Shrink};

use super::group::Run;
use super::{Context, Message, content};
use crate::config::messages::Timestamps;
use crate::theme;
use crate::widget::{Element, avatar};

/// A run under one header, with the avatar in a left gutter and every following
/// message aligned to the same indent.
pub fn run<'a>(run: &Run<'a>, context: &Context<'_>) -> Element<'a, Message> {
    let colors = theme::colors();
    let name = context.state.sender_name(run.sender);
    let color = if run.sender == context.state.aci {
        colors.text
    } else {
        theme::accent(run.sender.as_bytes())
    };
    let first = &run.messages[0];
    let spacing = context.spacing();

    let mut header = row![
        text(name.clone())
            .size(spacing.body)
            .color(color)
            .font(theme::FONT_BOLD)
            .height(Shrink),
    ]
    .spacing(6)
    .align_y(Center);
    if context.timestamps() != Timestamps::Never {
        header = header.push(stamp(first.timestamp(), spacing.small));
    }

    let bodies = run.messages.iter().map(|message| {
        content::one(
            message,
            context,
            content::quote_block(message, context),
            Vec::new(),
            content::spans(message, context),
        )
    });

    row![
        container(avatar::view(
            &name,
            color,
            spacing.avatar,
            context.avatar(run.sender),
        ))
        .width(spacing.gutter),
        column(std::iter::once(header.into()).chain(bodies)).spacing(spacing.within_run),
    ]
    .into()
}

/// The gutter time, with the full date behind a hover so it is available without
/// costing a line.
fn stamp<'a>(timestamp: u64, size: f32) -> Element<'a, Message> {
    tooltip(
        text(content::clock(timestamp))
            .size(size)
            .color(theme::colors().muted)
            .height(Shrink),
        container(
            text(content::full_timestamp(timestamp))
                .size(size)
                .height(Shrink),
        )
        .padding([3, 6])
        .style(|theme| theme::pane(theme, false)),
        tooltip::Position::Top,
    )
    .delay(std::time::Duration::from_millis(500))
    .into()
}
