use gpui::prelude::*;
use gpui::{Div, SharedString, div, px};

use super::kit;
use crate::config::Theme;

/// The composer card. Sending arrives in the next phase; this is the shape it
/// lives in -- a rounded panel floating over the conversation with its controls
/// inside it, and a thin context strip beneath.
pub struct Composer<'a> {
    pub placeholder: String,
    pub typing: Option<String>,
    pub palette: &'a Theme,
}

impl Composer<'_> {
    pub fn render(self) -> Div {
        let palette = self.palette;
        let size = palette.typography.message_size;

        let field = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3p5()
            .py_3()
            .rounded(px(12.0))
            .bg(palette.elevated)
            .border_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(size))
                    .text_color(palette.text_muted)
                    .child(SharedString::from(self.placeholder)),
            )
            .child(icon("＋", palette))
            .child(send(palette));

        kit::measured()
            .flex()
            .flex_col()
            .gap_1p5()
            .px_4()
            .pb_4()
            .pt_2()
            .when_some(self.typing, |this, who| {
                this.child(
                    div()
                        .px_1()
                        .text_size(px(palette.typography.ui_size - 2.0))
                        .text_color(palette.text_muted)
                        .child(SharedString::from(who)),
                )
            })
            .child(field)
            .child(context(palette))
    }
}

/// The strip under the composer, carrying whatever is true about where this
/// message is going rather than another row of buttons.
fn context(palette: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px_1()
        .text_size(px(palette.typography.ui_size - 3.0))
        .text_color(palette.text_muted)
        .child(div().child("Signal"))
        .child(div().child("Enter to send · Shift+Enter for a new line"))
}

fn icon(glyph: &'static str, palette: &Theme) -> Div {
    div()
        .flex_none()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .text_size(px(13.0))
        .text_color(palette.text_muted)
        .child(glyph)
}

/// The one bright thing on the screen, so the eye knows where the action is.
fn send(palette: &Theme) -> Div {
    div()
        .flex_none()
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(palette.accent)
        .text_size(px(13.0))
        .text_color(palette.on_accent)
        .child("↑")
}
