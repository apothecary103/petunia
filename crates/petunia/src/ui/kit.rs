//! Petunia's own primitives. The widget library supplies the window root and
//! the text input; everything with a visual opinion is built here so the look
//! is one decision rather than a per-view accident.

use gpui::prelude::*;
use gpui::{App, Div, Hsla, MouseButton, SharedString, Stateful, Window, div, px};
use gpui_component::{Icon, IconName};

use petunia_config::Theme;

/// An icon at a weight that reads as chrome rather than as content.
pub fn icon(name: IconName, size: f32, tint: Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(tint)
}

/// The rounding used everywhere. One value, so nothing drifts.
pub const RADIUS: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;

/// The two weights above regular that macOS actually draws.
///
/// gpui asks font-kit for a CSS weight and font-kit matches per CSS Fonts 3.
/// The system family reports its faces at CoreText's own weights, and the
/// conversion lands Medium on 530 rather than 500 -- so a request for 500 finds
/// no exact match, hits the rule that checks 400 first, and rasterises as
/// Regular. `FontWeight::MEDIUM` is therefore invisible here. Semibold (600) and
/// Bold (700) land exactly, and are what AppKit itself hands out for emphasis.
pub const EMPHASIS: gpui::FontWeight = gpui::FontWeight::SEMIBOLD;
pub const STRONG: gpui::FontWeight = gpui::FontWeight::BOLD;

/// A row that can be picked: quiet by default, filled when it is the one you
/// are on, outlined so selecting it never shifts the layout.
pub fn row(id: impl Into<gpui::ElementId>, selected: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_start()
        .gap_2p5()
        .px_2()
        .py_2()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(if selected {
            theme.border
        } else {
            gpui::transparent_black()
        })
        .when(selected, |this| this.bg(theme.elevated))
        .when(!selected, |this| this.hover(|this| this.bg(theme.hover)))
}

/// A section label. Small, muted, and never competing with the rows under it.
/// Upper case, because gpui has no letter-spacing and case is the only other
/// thing that makes a line this small read as a heading rather than as text.
pub fn section(label: impl Into<SharedString>, theme: &Theme) -> Div {
    heading(None, label, theme)
}

/// A section label with an icon, for the ones that are a kind of thing rather
/// than simply the rest.
pub fn heading(icon: Option<IconName>, label: impl Into<SharedString>, theme: &Theme) -> Div {
    let label = label.into();

    div()
        .flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .pt_3()
        .pb_1p5()
        .when_some(icon, |this, icon| {
            this.child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(self::icon(icon, 11.0, theme.text_muted)),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(theme.typography.ui_size - 3.0))
                .font_weight(EMPHASIS)
                .text_color(theme.text_muted)
                .child(SharedString::from(label.to_uppercase())),
        )
}

/// A square, quiet, hover-lit control. The reference has no bordered buttons in
/// its chrome, only glyphs that light up.
pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    name: IconName,
    theme: &Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .hover(|this| this.bg(theme.hover))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(icon(name, 15.0, theme.text_muted))
}

/// A small rounded label. Used for counts, states and anything annotating
/// something else rather than standing alone.
pub fn chip(label: impl Into<SharedString>, tint: Hsla, theme: &Theme) -> Div {
    chip_sized(label, tint, theme.typography.ui_size - 4.0)
}

/// The same chip at a size the caller picks, for the message list -- which is
/// scaled by density and by `scale`, so a chip drawn there at the interface size
/// would be the one thing in the run that does not follow.
pub fn chip_sized(label: impl Into<SharedString>, tint: Hsla, size: f32) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded_full()
        .bg(tinted(tint))
        .text_size(px(size))
        .text_color(tint)
        .child(label.into())
}

/// A filled dot, for "something happened here" with nothing worth counting.
pub fn dot(tint: Hsla) -> Div {
    div().flex_none().size(px(6.0)).rounded_full().bg(tint)
}

/// The reading column. Prose that runs the full width of a wide window is
/// unreadable, so the conversation is capped and centred like the reference.
pub const MEASURE: f32 = 760.0;

pub fn measured() -> Div {
    div().w_full().max_w(px(MEASURE)).mx_auto()
}

/// A colour at the strength a fill wants rather than the strength text wants.
pub fn tinted(color: Hsla) -> Hsla {
    Hsla { a: 0.16, ..color }
}

/// The card a dialog sits in, over a scrim that dims what it is covering.
/// Clicking the scrim is how a dialog is dismissed, so the card stops the click
/// from reaching it.
pub fn dialog(width: f32, theme: &Theme) -> Div {
    div()
        .w(px(width))
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded(px(RADIUS_LG))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub fn scrim(theme: &Theme) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(Hsla {
            a: 0.6,
            ..theme.background
        })
}

/// How a button in a dialog reads. `Danger` is filled in the warning colour
/// rather than the accent, because the accent is what "send" is and a delete must
/// not look like the safe thing to click.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Primary,
    Danger,
    Quiet,
}

pub fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    intent: Intent,
    theme: &Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let fill = match intent {
        Intent::Primary => Some(theme.accent),
        Intent::Danger => Some(theme.danger),
        Intent::Quiet => None,
    };

    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded(px(RADIUS))
        .cursor_pointer()
        .when_some(fill, |this, fill| this.bg(fill))
        .when(fill.is_none(), |this| {
            this.border_1()
                .border_color(theme.border)
                .hover(|this| this.bg(theme.hover))
        })
        .text_size(px(theme.typography.ui_size))
        .text_color(match fill {
            Some(_) => theme.on_accent,
            None => theme.text_dim,
        })
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label.into())
}
