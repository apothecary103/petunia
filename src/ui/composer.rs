use gpui::prelude::*;
use gpui::{Div, SharedString, div, px};
use gpui_component::IconName;

use super::kit;
use crate::config::Theme;

/// The composer card. Sending arrives in the next phase; this is the shape it
/// lives in -- a rounded panel floating over the conversation with its controls
/// inside it, and a thin context strip beneath.
pub struct Composer<'a> {
    pub placeholder: String,
    pub typing: Option<String>,
    pub palette: &'a Theme,
    pub formatting: bool,
    pub on_formatting: Box<dyn Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App)>,
}

/// Signal's own formatting. Each button is drawn in the style it applies, so it
/// shows what it does rather than needing an icon to say so -- the icon set has
/// no bold or italic, and the box-drawing glyph a spoiler wanted is simply
/// absent from the system font.
#[derive(Clone, Copy)]
enum Mark {
    Bold,
    Italic,
    Strikethrough,
    Monospace,
    Spoiler,
}

const MARKS: [Mark; 5] = [
    Mark::Bold,
    Mark::Italic,
    Mark::Strikethrough,
    Mark::Monospace,
    Mark::Spoiler,
];

impl Mark {
    fn id(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Strikethrough => "strikethrough",
            Self::Monospace => "monospace",
            Self::Spoiler => "spoiler",
        }
    }
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
            .child(
                div()
                    .id("formatting")
                    .flex_none()
                    .size(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(7.0))
                    .when(self.formatting, |this| this.bg(palette.active))
                    .hover(|this| this.bg(palette.hover))
                    .text_size(px(12.0))
                    .text_color(if self.formatting {
                        palette.text_dim
                    } else {
                        palette.text_muted
                    })
                    .on_mouse_down(gpui::MouseButton::Left, self.on_formatting)
                    .child("Aa"),
            )
            .child(kit::icon(IconName::Plus, 15.0, palette.text_muted))
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
            .when(self.formatting, |this| this.child(toolbar(palette)))
            .child(field)
            .child(context(palette))
    }
}

/// Signal's formatting marks, revealed rather than always present: a chat box
/// is not a word processor, and the reference keeps its controls inside the
/// field until asked.
fn toolbar(palette: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .p_1()
        .rounded(px(kit::RADIUS))
        .bg(palette.elevated)
        .border_1()
        .border_color(palette.border)
        .children(MARKS.iter().map(|mark| {
            // A fixed square with everything centred inside it, so a glyph and
            // an icon sit on the same baseline instead of drifting apart.
            let button = div()
                .id(mark.id())
                .size(px(26.0))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .hover(|this| this.bg(palette.hover))
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text_dim);

            match mark {
                Mark::Bold => button.font_weight(gpui::FontWeight::BOLD).child("B"),
                Mark::Italic => button.italic().child("I"),
                Mark::Strikethrough => button.line_through().child("S"),
                Mark::Monospace => button
                    .font_family(palette.typography.mono.clone())
                    .child("M"),
                Mark::Spoiler => {
                    button.child(kit::icon(IconName::EyeOff, 15.0, palette.text_dim))
                }
            }
        }))
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
