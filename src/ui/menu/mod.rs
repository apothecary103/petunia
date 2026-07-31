//! Right-click menus.
//!
//! Built here rather than on the widget library's `PopupMenu`, whose items are
//! gpui actions dispatched along the focus path. An action is a type, so
//! "archive *this* conversation" would mean a type per verb carrying a thread,
//! registered in the keymap, to reach a handler that already knows the thread
//! because it just drew the row. A closure is what that is.

use gpui::prelude::*;
use gpui::{Context, MouseButton, Pixels, Point, SharedString, Window, div, px};
use gpui_component::IconName;

use super::kit;
use crate::config::Theme;
use crate::theme::ActivePalette;

pub mod message;
pub mod thread;

pub struct Dismissed;

/// What an entry does when it is picked.
pub type Chosen = Box<dyn Fn(&mut Window, &mut gpui::App)>;

impl gpui::EventEmitter<Dismissed> for Menu {}

/// What a menu is made of. A separator is a variant rather than a flag so two
/// in a row, or one at the end, cannot be drawn.
pub enum Item {
    Entry {
        label: SharedString,
        icon: Option<IconName>,
        /// Shown with a tick. `None` for anything that is not a toggle.
        checked: Option<bool>,
        danger: bool,
        act: Chosen,
    },
    Separator,
    /// A heading, for a group of entries that needs one.
    Label(SharedString),
}

impl Item {
    pub fn new(
        label: impl Into<SharedString>,
        act: impl Fn(&mut Window, &mut gpui::App) + 'static,
    ) -> Self {
        Self::Entry {
            label: label.into(),
            icon: None,
            checked: None,
            danger: false,
            act: Box::new(act),
        }
    }

    pub fn icon(mut self, name: IconName) -> Self {
        if let Self::Entry { icon, .. } = &mut self {
            *icon = Some(name);
        }
        self
    }

    /// Marks this as a toggle, which draws a tick when it is on.
    pub fn checked(mut self, on: bool) -> Self {
        if let Self::Entry { checked, .. } = &mut self {
            *checked = Some(on);
        }
        self
    }

    /// Destructive, and coloured as such.
    pub fn danger(mut self) -> Self {
        if let Self::Entry { danger, .. } = &mut self {
            *danger = true;
        }
        self
    }
}

pub struct Menu {
    items: Vec<Item>,
    at: Point<Pixels>,
    focus: gpui::FocusHandle,
}

/// Roughly what one entry occupies, for keeping the menu on screen. Measuring
/// would need a layout pass, and a menu that is one row too high is a worse
/// outcome than one estimated a few pixels out.
const ROW: f32 = 28.0;
const SEPARATOR: f32 = 9.0;
const WIDTH: f32 = 210.0;

impl Menu {
    pub fn new(items: Vec<Item>, at: Point<Pixels>, cx: &mut Context<Self>) -> Self {
        Self {
            items,
            at,
            focus: cx.focus_handle(),
        }
    }

    fn height(&self) -> f32 {
        self.items
            .iter()
            .map(|item| match item {
                Item::Separator => SEPARATOR,
                _ => ROW,
            })
            .sum::<f32>()
            + 8.0
    }
}

impl gpui::Focusable for Menu {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let viewport = window.viewport_size();

        // Flipped rather than clipped when it would run off an edge, which is
        // what every menu on every platform does.
        let height = self.height();
        let left = match f32::from(self.at.x) + WIDTH > f32::from(viewport.width) {
            true => (f32::from(self.at.x) - WIDTH).max(4.0),
            false => f32::from(self.at.x),
        };
        let top = match f32::from(self.at.y) + height > f32::from(viewport.height) {
            true => (f32::from(self.at.y) - height).max(4.0),
            false => f32::from(self.at.y),
        };

        let rows = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| row(index, item, &palette, cx))
            .collect::<Vec<_>>();

        div()
            .id("menu-backdrop")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            // Any click anywhere else closes it, including a right-click meant
            // to open another one.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(WIDTH))
                    .flex()
                    .flex_col()
                    .py_1()
                    .rounded(px(kit::RADIUS))
                    .bg(palette.elevated)
                    .border_1()
                    .border_color(palette.border)
                    .shadow_lg()
                    .children(rows),
            )
    }
}

fn row(
    index: usize,
    item: &Item,
    palette: &Theme,
    cx: &mut Context<Menu>,
) -> gpui::AnyElement {
    match item {
        Item::Separator => div()
            .my_1()
            .mx_2()
            .h_px()
            .bg(palette.border)
            .into_any_element(),
        Item::Label(label) => div()
            .px_3()
            .pt_1p5()
            .pb_0p5()
            .text_size(px(palette.typography.ui_size - 3.0))
            .text_color(palette.text_muted)
            .child(label.clone())
            .into_any_element(),
        Item::Entry {
            label,
            icon,
            checked,
            danger,
            ..
        } => {
            let tint = match danger {
                true => palette.danger,
                false => palette.text_dim,
            };

            div()
                .id(SharedString::from(format!("menu-{index}")))
                .flex()
                .items_center()
                .gap_2()
                .mx_1()
                .px_2()
                .h(px(ROW - 2.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .hover(|this| this.bg(palette.hover))
                .text_size(px(palette.typography.ui_size))
                .text_color(tint)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this: &mut Menu, _, window, cx| {
                        if let Some(Item::Entry { act, .. }) = this.items.get(index) {
                            act(window, cx);
                        }
                        cx.emit(Dismissed);
                    }),
                )
                .when_some(icon.clone(), |this, icon| {
                    this.child(kit::icon(icon, 14.0, tint))
                })
                .when(icon.is_none() && checked.is_some(), |this| {
                    this.child(match checked {
                        Some(true) => kit::icon(IconName::Check, 14.0, tint).into_any_element(),
                        _ => div().size(px(14.0)).into_any_element(),
                    })
                })
                .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                .when(icon.is_some() && *checked == Some(true), |this| {
                    this.child(kit::icon(IconName::Check, 13.0, palette.accent))
                })
                .into_any_element()
        }
    }
}
