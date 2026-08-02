//! The controls that appear over a message when the pointer is on it.

use gpui::prelude::*;
use gpui::{Div, MouseButton, SharedString, div, px};
use gpui_component::IconName;

use super::act::{Act, Dispatch};
use petunia_config::Theme;
use petunia_data::Message;
use crate::ui::kit;

/// The reactions worth one click. Signal offers six and puts the rest behind a
/// picker; these are the six.
const QUICK: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

/// Wraps a message in its hover controls. The bar is drawn inside the message's
/// own box and only becomes visible when the group is hovered, so it costs no
/// layout and never pushes the conversation around.
pub fn with_actions(
    body: Div,
    message: &Message,
    own: bool,
    copied: bool,
    theme: &Theme,
    act: &Dispatch,
) -> gpui::Stateful<Div> {
    if !message.is_addressable() {
        return body.id(SharedString::from(format!("body-{}", message.timestamp())));
    }

    let group = SharedString::from(format!("message-{}", message.timestamp()));
    let id = message.id;
    let raise = act.clone();

    div()
        .id(SharedString::from(format!("body-{}", message.timestamp())))
        .relative()
        .group(group.clone())
        .on_mouse_down(
            MouseButton::Right,
            move |event: &gpui::MouseDownEvent, window, cx| {
                raise(Act::Menu(id, event.position), window, cx)
            },
        )
        .child(body)
        .child(
            div()
                .absolute()
                .top(px(-14.0))
                .right_0()
                .invisible()
                // Whatever else it is doing, the bar stays up while it is saying
                // the text was taken: an answer that appears under a pointer that
                // has already moved on is an answer to nobody.
                .when(copied, |this| this.visible())
                .group_hover(group, |this| this.visible())
                .child(bar(message, own, copied, theme, act)),
        )
}

fn bar(message: &Message, own: bool, copied: bool, theme: &Theme, act: &Dispatch) -> Div {
    let id = message.id;
    let has_text = message.text().is_some_and(|text| !text.is_empty());

    let mut bar = div()
        .flex()
        .items_center()
        .gap_0p5()
        .p_0p5()
        .rounded(px(kit::RADIUS))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border);

    for emoji in QUICK {
        bar = bar.child(emoji_button(emoji, id, theme, act));
    }

    bar = bar
        .child(divider(theme))
        .child(button(
            format!("reply-{}", id.timestamp),
            IconName::Undo,
            "Reply",
            theme,
            act.clone(),
            Act::Reply(id),
        ))
        .child(button(
            format!("forward-{}", id.timestamp),
            IconName::Redo,
            "Forward",
            theme,
            act.clone(),
            Act::Forward(id),
        ));

    // A copy is a thing that happens somewhere else, so the only place it can be
    // reported is here: the icon becomes a check in the accent for a moment. A
    // control that answers nothing is a control you press twice.
    if has_text {
        let (icon, tooltip, tint) = match copied {
            true => (IconName::Check, "Copied", theme.accent),
            false => (IconName::Copy, "Copy text", theme.text_muted),
        };
        bar = bar.child(marked(
            format!("copy-{}", id.timestamp),
            icon,
            tooltip,
            tint,
            theme,
            act.clone(),
            Act::Copy(id),
        ));
    }

    // Signal permits neither editing nor deleting someone else's message, so
    // drawing the controls at all would be a lie about what they do.
    if own {
        if has_text {
            bar = bar.child(button(
                format!("edit-{}", id.timestamp),
                IconName::Replace,
                "Edit",
                theme,
                act.clone(),
                Act::Edit(id),
            ));
        }
        bar = bar.child(button(
            format!("delete-{}", id.timestamp),
            IconName::Delete,
            "Delete",
            theme,
            act.clone(),
            Act::Delete(id),
        ));
    }

    bar
}

fn divider(theme: &Theme) -> Div {
    div().w_px().h(px(16.0)).mx_0p5().bg(theme.border)
}

fn emoji_button(
    emoji: &'static str,
    id: petunia_data::MessageId,
    theme: &Theme,
    act: &Dispatch,
) -> gpui::Stateful<Div> {
    let act = act.clone();

    square(format!("react-{emoji}-{}", id.timestamp), theme)
        .text_size(px(14.0))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            act(Act::React(id, emoji.to_string()), window, cx)
        })
        .child(emoji)
}

fn button(
    id: String,
    icon: IconName,
    tooltip: &'static str,
    theme: &Theme,
    act: Dispatch,
    what: Act,
) -> gpui::Stateful<Div> {
    marked(id, icon, tooltip, theme.text_muted, theme, act, what)
}

/// The same, in a colour of its own -- which only the copy button asks for, to
/// say for a moment that it did something.
fn marked(
    id: String,
    icon: IconName,
    tooltip: &'static str,
    tint: gpui::Hsla,
    theme: &Theme,
    act: Dispatch,
    what: Act,
) -> gpui::Stateful<Div> {
    square(id, theme)
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            act(what.clone(), window, cx)
        })
        .child(kit::icon(icon, 14.0, tint))
}

fn square(id: impl Into<SharedString>, theme: &Theme) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .size(px(24.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .cursor_pointer()
        .hover(|this| this.bg(theme.hover))
}
