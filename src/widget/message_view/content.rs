use chrono::{DateTime, Local};
use iced::widget::text::Span;
use iced::widget::{Space, button, column, container, image, row, rich_text, text};
use iced::{Center, Fill, Shrink};

use super::{Context, Link, Message};
use crate::data::attachment::{Attachment, Blob, Kind};
use crate::data::message::{Content, Reaction, Update};
use crate::data::{self, Status};
use crate::theme;
use crate::widget::Element;

const THUMBNAIL: f32 = 34.0;

/// The body of a message as spans, so each layout can decide what to put around
/// them. Returning spans rather than a finished element is what lets IRC mode
/// keep the prefix and body in one `rich_text` so wrapped lines hang correctly.
pub fn spans<'a>(message: &'a data::Message, context: &Context<'_>) -> Vec<Span<'a, Link>> {
    let colors = theme::colors();

    let mut spans = match &message.content {
        Content::Text { body, ranges } if !body.is_empty() => {
            super::format::spans(message.id, body, ranges, context.state, context.revealed)
        }
        Content::Deleted => vec![
            Span::new("This message was deleted")
                .color(colors.muted)
                .font(theme::FONT_ITALIC),
        ],
        // An attachment-only message has no body; its media carries it.
        Content::Text { .. } | Content::Sticker(_) => Vec::new(),
        Content::Update(update) => vec![Span::new(describe(update)).color(colors.dim)],
    };

    if message.edited.is_some() {
        spans.push(
            Span::new(" (edited)")
                .color(colors.muted)
                .size(context.spacing().small),
        );
    }
    spans
}

fn describe(update: &Update) -> String {
    match update {
        Update::ExpireTimer { seconds: 0 } => "Disappearing messages off".into(),
        Update::ExpireTimer { seconds } => {
            format!("Disappearing messages set to {}", duration(*seconds))
        }
    }
}

fn duration(seconds: u32) -> String {
    match seconds {
        s if s % 604_800 == 0 => format!("{} weeks", s / 604_800),
        s if s % 86_400 == 0 => format!("{} days", s / 86_400),
        s if s % 3_600 == 0 => format!("{} hours", s / 3_600),
        s if s % 60 == 0 => format!("{} minutes", s / 60),
        s => format!("{s} seconds"),
    }
}

/// Everything that hangs below the body: media, then the reaction row. The quote
/// is deliberately not here -- it belongs *above* the message it answers, and the
/// two layouts frame it differently.
pub fn extras<'a>(
    message: &'a data::Message,
    context: &Context<'_>,
) -> Vec<Element<'a, Message>> {
    let mut extras = Vec::new();

    if let Content::Sticker(sticker) = &message.content {
        extras.push(self::sticker(sticker, message.timestamp(), context));
    }
    for attached in message.attachments.iter() {
        extras.push(attachment(attached, message.timestamp(), context));
    }
    if !message.reactions.is_empty() {
        extras.push(reactions(&message.reactions, message, context));
    }
    extras
}

/// A sticker is an image with no bubble, no chip and no caption, drawn at a size
/// of its own rather than at the inline-media cap.
fn sticker<'a>(
    sticker: &'a data::message::Sticker,
    timestamp: u64,
    context: &Context<'_>,
) -> Element<'a, Message> {
    let edge = context.spacing().sticker;
    let Some(attached) = &sticker.image else {
        return placeholder(sticker, edge);
    };

    match &attached.blob {
        // A fixed square, not a cap: a sticker whose bytes will not decode must
        // still hold its place rather than silently collapsing to nothing.
        Blob::Cached(path) => container(image(path)).center(edge).into(),
        // A sticker that has not arrived still occupies its square, so the
        // timeline does not jump when it does.
        Blob::Downloading(_) => placeholder(sticker, edge),
        Blob::Missing | Blob::Failed(_) => button(placeholder(sticker, edge))
            .on_press(Message::Download(timestamp, attached.id.clone()))
            .padding(0)
            .style(|_theme, _status| button::Style::default())
            .into(),
    }
}

fn placeholder<'a>(sticker: &data::message::Sticker, edge: f32) -> Element<'a, Message> {
    container(
        text(sticker.emoji.clone().unwrap_or_else(|| "🏷".into()))
            .size(edge * 0.3)
            .height(Shrink),
    )
    .center(edge)
    .into()
}

/// The message being replied to, shown above the reply as a block. Not yet
/// clickable: jumping to a message outside the loaded page needs paging that does
/// not exist, and a control that silently does nothing is worse than none.
pub fn quote_block<'a>(
    message: &'a data::Message,
    context: &Context<'_>,
) -> Option<Element<'a, Message>> {
    let quote = message.quote.as_ref()?;
    let colors = theme::colors();
    let small = context.spacing().small;
    let author = theme::accent(quote.id.sender.as_bytes());

    let mut preview = row![
        container(Space::new().width(2).height(Fill)).style(move |_: &iced::Theme| {
            container::Style {
                background: Some(iced::Background::Color(author)),
                border: iced::border::rounded(1),
                ..container::Style::default()
            }
        }),
        column![
            text(context.state.sender_name(quote.id.sender))
                .size(small)
                .color(author)
                .font(theme::FONT_BOLD)
                .height(Shrink),
            text(quote_body(quote)).size(small).color(colors.dim).height(Shrink),
        ]
        .spacing(1),
    ]
    .spacing(6);

    if let Some(thumbnail) = &quote.thumbnail
        && let Blob::Cached(path) = &thumbnail.blob
    {
        preview = preview.push(image(path).height(THUMBNAIL).width(THUMBNAIL));
    }

    Some(
        container(preview)
            .padding([3, 5])
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(theme::colors().sunken)),
                border: iced::border::rounded(4),
                ..container::Style::default()
            })
            .into(),
    )
}

/// The same reply, one line, for a layout whose whole point is one line per
/// message. A block here would cost three lines and out-shout the reply itself.
pub fn quote_line<'a>(
    message: &'a data::Message,
    context: &Context<'_>,
) -> Option<Element<'a, Message>> {
    let quote = message.quote.as_ref()?;
    let colors = theme::colors();
    let small = context.spacing().small;
    let author = theme::accent(quote.id.sender.as_bytes());

    Some(
        row![
            text("↳").size(small).color(colors.muted).height(Shrink),
            text(context.state.sender_name(quote.id.sender))
                .size(small)
                .color(author)
                .font(theme::FONT_BOLD)
                .height(Shrink),
            text(quote_body(quote)).size(small).color(colors.muted).height(Shrink),
        ]
        .spacing(5)
        .align_y(Center)
        .into(),
    )
}

/// One line, whatever the original was: a quote is a reminder, not a copy.
fn quote_body(quote: &data::message::Quote) -> String {
    const LIMIT: usize = 120;

    if quote.body.is_empty() {
        return "Attachment".to_string();
    }
    let body = quote.body.replace('\n', " ");
    match body.char_indices().nth(LIMIT) {
        Some((at, _)) => format!("{}…", &body[..at]),
        None => body,
    }
}

/// Enough to see that media arrived and to fetch whatever auto-download skipped.
fn attachment<'a>(
    attached: &'a Attachment,
    timestamp: u64,
    context: &Context<'_>,
) -> Element<'a, Message> {
    let colors = theme::colors();
    let small = context.spacing().small;
    let chip = |content: String| text(content).size(small).color(colors.dim).height(Shrink);
    let fetch = |label: String, color| {
        button(text(label).size(small).height(Shrink).color(color))
            .on_press(Message::Download(timestamp, attached.id.clone()))
            .padding(0)
            .style(theme::pane_control)
            .into()
    };

    match (&attached.blob, &attached.kind) {
        (Blob::Cached(path), Kind::Image { .. }) => button(bounded(
            image(path),
            context.image_max_width,
            context.image_max_height,
        ))
        .on_press(Message::OpenAttachment(path.clone()))
        .padding(0)
        .style(|_theme, _status| button::Style::default())
        .into(),
        (Blob::Cached(path), _) => button(chip(format!("[{} — open]", describe_file(attached))))
            .on_press(Message::OpenAttachment(path.clone()))
            .padding(0)
            .style(theme::pane_control)
            .into(),
        (Blob::Downloading(_), _) => chip(format!("[{} — downloading…]", describe_file(attached)))
            .into(),
        (Blob::Missing, _) => fetch(
            format!("[{} — download]", describe_file(attached)),
            colors.dim,
        ),
        (Blob::Failed(error), _) => fetch(
            format!("[{} — {error}, retry]", describe_file(attached)),
            colors.danger,
        ),
    }
}

/// iced measures an image's intrinsic size in *pixels* and treats them as logical
/// units, so an unbounded 3000px screenshot lays out 3000 units wide. Capping
/// both axes and letting the image shrink to fit scales it down at its own aspect
/// ratio, and never scales it up.
fn bounded<'a>(
    image: iced::widget::Image<iced::widget::image::Handle>,
    max_width: f32,
    max_height: f32,
) -> Element<'a, Message> {
    container(image)
        .max_width(max_width)
        .max_height(max_height)
        .into()
}

fn describe_file(attached: &Attachment) -> String {
    let name = attached
        .file_name
        .clone()
        .unwrap_or_else(|| attached.content_type.clone());

    match attached.size {
        0 => name,
        size => format!("{name}, {}", bytes(size)),
    }
}

fn bytes(size: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = size as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{size:.0} {}", UNITS[unit]),
        _ => format!("{size:.1} {}", UNITS[unit]),
    }
}

/// One chip per distinct emoji with its count, tinted when the reaction includes
/// you. Clicking a chip you are part of removes your reaction.
fn reactions<'a>(
    reactions: &'a [Reaction],
    message: &'a data::Message,
    context: &Context<'_>,
) -> Element<'a, Message> {
    let mut distinct: Vec<(&str, usize, bool)> = Vec::new();

    for reaction in reactions {
        let mine = reaction.author == context.state.aci;
        match distinct
            .iter_mut()
            .find(|(emoji, _, _)| *emoji == reaction.emoji)
        {
            Some((_, count, includes_me)) => {
                *count += 1;
                *includes_me |= mine;
            }
            None => distinct.push((&reaction.emoji, 1, mine)),
        }
    }

    let chips = distinct.into_iter().map(|(emoji, count, mine)| {
        let label = if count > 1 {
            format!("{emoji} {count}")
        } else {
            emoji.to_string()
        };
        button(text(label).size(context.spacing().small).height(Shrink))
            .on_press(Message::React(message.id, emoji.to_string(), mine))
            .padding([1, 5])
            .style(move |_theme, status| {
                let colors = theme::colors();
                let hovered = matches!(
                    status,
                    button::Status::Hovered | button::Status::Pressed
                );
                button::Style {
                    background: Some(iced::Background::Color(if mine {
                        iced::Color { a: 0.25, ..colors.accent }
                    } else if hovered {
                        colors.border
                    } else {
                        colors.sunken
                    })),
                    text_color: if mine { colors.accent } else { colors.dim },
                    border: iced::Border {
                        width: 1.0,
                        color: if mine { colors.accent } else { colors.border },
                        radius: iced::border::radius(9),
                    },
                    ..button::Style::default()
                }
            })
            .into()
    });

    row(chips).spacing(3).align_y(Center).into()
}

/// Trailing status marker for our own messages. A single glyph, because a word
/// on every line is noise once you trust it.
fn status_span<'a>(status: Status, context: &Context<'_>) -> Span<'a, Link> {
    let (glyph, color) = status_glyph(status);
    Span::new(format!(" {glyph}"))
        .color(color)
        .size(context.spacing().small)
}

/// The same marker for a message with no body to hang it on.
fn status_mark<'a>(status: Status, context: &Context<'_>) -> Element<'a, Message> {
    let (glyph, color) = status_glyph(status);
    text(glyph)
        .size(context.spacing().small)
        .color(color)
        .height(Shrink)
        .into()
}

fn status_glyph(status: Status) -> (&'static str, iced::Color) {
    let colors = theme::colors();
    match status {
        Status::Sending => ("·", colors.muted),
        Status::Failed => ("!", colors.danger),
        Status::Sent => ("✓", colors.muted),
        Status::Delivered => ("✓✓", colors.muted),
        Status::Read | Status::Viewed => ("✓✓", colors.accent),
    }
}

pub fn clock(timestamp: u64) -> String {
    local(timestamp).format("%H:%M").to_string()
}

pub fn full_timestamp(timestamp: u64) -> String {
    local(timestamp).format("%A %e %B %Y at %H:%M:%S").to_string()
}

fn local(timestamp: u64) -> DateTime<Local> {
    DateTime::from_timestamp_millis(timestamp as i64)
        .unwrap_or_default()
        .with_timezone(&Local)
}

/// Used by both frames for the body row, so the two cannot drift.
pub fn body<'a>(spans: Vec<Span<'a, Link>>, context: &Context<'_>) -> Element<'a, Message> {
    rich_text(spans)
        .size(context.spacing().body)
        .on_link_click(Message::Link)
        .into()
}

/// One message, assembled: the reply it answers above, the body, then its media
/// and reactions. Shared so a sticker or a quote cannot appear in one layout and
/// go missing in the other -- only the framing of each part differs.
///
/// `prefix` is whatever the layout puts before the body in the same `rich_text`
/// -- the time and sender in IRC mode, nothing in grouped mode. It is kept
/// separate from `written` so that "this message has no body" stays answerable.
pub fn one<'a>(
    message: &'a data::Message,
    context: &Context<'_>,
    quote: Option<Element<'a, Message>>,
    prefix: Vec<Span<'a, Link>>,
    written: Vec<Span<'a, Link>>,
) -> Element<'a, Message> {
    let bodied = !written.is_empty();
    let mut spans = prefix;
    spans.extend(written);

    // The status rides on the body when there is one, and trails the media when
    // there is not -- a sticker with a tick floating above it on a line of its
    // own reads as a message that failed to render.
    if let Some(status) = message.status
        && bodied
    {
        spans.push(status_span(status, context));
    }

    // An empty `rich_text` still claims a line, so a body-less message must not
    // build one at all.
    let body = (!spans.is_empty()).then(|| body(spans, context));
    let trailing = message
        .status
        .filter(|_| !bodied)
        .map(|status| status_mark(status, context));

    let parts: Vec<Element<'a, Message>> = quote
        .into_iter()
        .chain(body)
        .chain(extras(message, context))
        .chain(trailing)
        .collect();

    let rendered = match <[_; 1]>::try_from(parts) {
        Ok([only]) => only,
        Err(parts) => column(parts).spacing(3).into(),
    };
    with_actions(message, context, rendered)
}

/// Wraps a message so its actions appear on hover. Without this, replying,
/// reacting and editing have no entry point at all.
fn with_actions<'a>(
    message: &'a data::Message,
    context: &Context<'_>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    if matches!(message.content, Content::Deleted | Content::Update(_)) {
        return content;
    }

    let mine = message.sender() == context.state.aci;
    let mut actions = row![
        action("↩", Message::Reply(message.id)),
        action("👍", Message::React(message.id, "👍".into(), false)),
    ]
    .spacing(1);

    if let Some(body) = message.text().filter(|body| !body.is_empty()) {
        actions = actions.push(action("copy", Message::Copy(body.to_string())));
    }
    // Signal only lets you edit or delete your own messages.
    if mine {
        if message.text().is_some() {
            actions = actions.push(action("edit", Message::Edit(message.id)));
        }
        actions = actions.push(action("del", Message::Delete(message.id)));
    }

    iced::widget::hover(
        content,
        container(
            container(actions)
                .padding([1, 3])
                .style(|theme| theme::pane(theme, false)),
        )
        .align_right(Fill)
        .height(Shrink),
    )
}

fn action<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    button(text(label).size(11).height(Shrink))
        .on_press(message)
        .padding([1, 4])
        .style(theme::pane_control)
        .into()
}
