//! The controls that appear over a message when the pointer is on it.

use gpui::prelude::*;
use gpui::{Div, MouseButton, SharedString, div, px};
use gpui_component::IconName;

use super::act::{Act, Dispatch};
use crate::config::Theme;
use crate::data::Message;
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
    theme: &Theme,
    act: &Dispatch,
) -> Div {
    if !message.is_addressable() {
        return body;
    }

    let group = SharedString::from(format!("message-{}", message.timestamp()));

    div()
        .relative()
        .group(group.clone())
        .child(body)
        .child(
            div()
                .absolute()
                .top(px(-14.0))
                .right_0()
                .invisible()
                .group_hover(group, |this| this.visible())
                .child(bar(message, own, theme, act)),
        )
}

fn bar(message: &Message, own: bool, theme: &Theme, act: &Dispatch) -> Div {
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

    bar = bar.child(divider(theme)).child(button(
        format!("reply-{}", id.timestamp),
        IconName::Undo,
        "Reply",
        theme,
        act.clone(),
        Act::Reply(id),
    ));

    if has_text {
        bar = bar.child(button(
            format!("copy-{}", id.timestamp),
            IconName::Copy,
            "Copy text",
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
    id: crate::data::MessageId,
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
    square(id, theme)
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            act(what.clone(), window, cx)
        })
        .child(kit::icon(icon, 14.0, theme.text_muted))
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
