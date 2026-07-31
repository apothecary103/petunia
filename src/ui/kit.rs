//! Petunia's own primitives. The widget library supplies the window root and
//! the text input; everything with a visual opinion is built here so the look
//! is one decision rather than a per-view accident.

use gpui::prelude::*;
use gpui::{App, Div, Hsla, MouseButton, SharedString, Stateful, Window, div, px};

use crate::config::Theme;

/// The rounding used everywhere. One value, so nothing drifts.
pub const RADIUS: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;

/// Vertical space between unrelated blocks, in the units the reference uses.
pub const GAP: f32 = 8.0;

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
pub fn section(label: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px_2()
        .pt_3()
        .pb_1p5()
        .child(
            div()
                .text_size(px(theme.typography.ui_size - 3.0))
                .text_color(theme.text_muted)
                .child(label.into()),
        )
}

/// A square, quiet, hover-lit control. The reference has no bordered buttons in
/// its chrome, only glyphs that light up.
pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    glyph: impl Into<SharedString>,
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
        .text_size(px(12.0))
        .text_color(theme.text_muted)
        .hover(|this| this.bg(theme.hover).text_color(theme.text_dim))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(glyph.into())
}

/// A small rounded label. Used for counts, states and anything annotating
/// something else rather than standing alone.
pub fn chip(label: impl Into<SharedString>, tint: Hsla, theme: &Theme) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded_full()
        .bg(tinted(tint))
        .text_size(px(theme.typography.ui_size - 4.0))
        .text_color(tint)
        .child(label.into())
}

/// A filled dot, for "something happened here" with nothing worth counting.
pub fn dot(tint: Hsla) -> Div {
    div().flex_none().size(px(6.0)).rounded_full().bg(tint)
}

/// A hairline. Borders in the reference are barely there.
pub fn rule(theme: &Theme) -> Div {
    div().h_px().flex_1().bg(theme.border)
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
