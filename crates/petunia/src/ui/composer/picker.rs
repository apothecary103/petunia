//! The panel over the composer, and the two tabs in it.
//!
//! One control opens it rather than two beside each other. A sticker and an
//! emoji are the same gesture — reach for a picture, put it in the message —
//! and two buttons for one gesture was two things to learn where. Stickers are
//! the default tab because that is the one with nothing else to reach it by:
//! an emoji can be typed, and a sticker cannot.

use gpui::prelude::*;
use gpui::{Div, MouseButton, SharedString, div, px};

use crate::ui::kit;
use petunia_config::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Stickers,
    Emoji,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Stickers => "Stickers",
            Self::Emoji => "Emoji",
        }
    }
}

pub const TABS: [Tab; 2] = [Tab::Stickers, Tab::Emoji];

/// Which tab was asked for. `Rc` because one closure is cloned into each of
/// them, as everything else the picker reports back is.
pub type Choose = std::rc::Rc<dyn Fn(&Tab, &mut gpui::Window, &mut gpui::App)>;

/// The panel itself: the tab strip, and whichever tab is showing under it.
pub fn panel(showing: Tab, theme: &Theme, on_tab: Choose, body: Div) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(kit::RADIUS))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .p_1()
                .border_b_1()
                .border_color(theme.border)
                .children(TABS.map(|tab| {
                    let on_tab = on_tab.clone();
                    strip(tab, tab == showing, theme, move |window, cx| {
                        on_tab(&tab, window, cx)
                    })
                })),
        )
        .child(body)
}

/// One tab. A word rather than an icon: there are two of them, and the whole
/// question the strip answers is which of two kinds of thing you are after.
fn strip(
    tab: Tab,
    selected: bool,
    theme: &Theme,
    on_click: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(SharedString::from(format!("tab-{}", tab.label())))
        .flex_none()
        .px_2p5()
        .py_1()
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(theme.typography.ui_size - 1.0))
        .when(selected, |this| {
            this.bg(theme.active)
                .text_color(theme.text)
                .font_weight(kit::EMPHASIS)
        })
        .when(!selected, |this| {
            this.text_color(theme.text_muted)
                .hover(|this| this.bg(theme.hover))
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_click(window, cx))
        .child(tab.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stickers first and by default: an emoji can be typed, and a sticker has
    /// nothing but this panel to reach it by.
    #[test]
    fn stickers_are_the_default_tab() {
        assert_eq!(Tab::default(), Tab::Stickers);
        assert_eq!(TABS.first(), Some(&Tab::Stickers));
    }
}
