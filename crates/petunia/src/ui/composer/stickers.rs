//! The sticker picker: a panel over the composer, one row of packs and a grid
//! of whatever the search matches.

use gpui::prelude::*;
use gpui::{Div, MouseButton, SharedString, div, px};

use petunia_config::Theme;
use petunia_data::stickers::Pack;
use crate::ui::{image, kit};

/// How large a sticker is drawn in the grid. Big enough to tell two cats apart,
/// small enough that a pack of sixty does not need scrolling twice.
const TILE: f32 = 68.0;
const COVER: f32 = 32.0;

/// Something the picker reports back. `Rc` because one closure is cloned into
/// every tile it draws.
pub type Pick<T> = std::rc::Rc<dyn Fn(&T, &mut gpui::Window, &mut gpui::App)>;

/// Which pack is showing, or all of them at once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Showing {
    #[default]
    Everything,
    Pack(Vec<u8>),
}

pub struct Picker<'a> {
    pub packs: &'a [Pack],
    pub showing: &'a Showing,
    pub query: &'a str,
    pub search: &'a gpui::Entity<gpui_component::input::InputState>,
    pub theme: &'a Theme,
    pub on_pack: Pick<Showing>,
    /// Which sticker was chosen: its pack, its id, its emoji and its file.
    pub on_pick: Pick<Chosen>,
}

#[derive(Debug, Clone)]
pub struct Chosen {
    pub pack_id: Vec<u8>,
    pub key: Vec<u8>,
    pub sticker_id: u32,
    pub emoji: Option<String>,
    pub path: std::path::PathBuf,
}

impl Picker<'_> {
    pub fn render(self) -> Div {
        let theme = self.theme;

        if self.packs.is_empty() {
            return shell(theme).child(
                div()
                    .p_4()
                    .text_size(px(theme.typography.ui_size - 1.0))
                    .text_color(theme.text_muted)
                    .child(
                        "No sticker packs yet. Tap a sticker someone sent you to add its pack.",
                    ),
            );
        }

        let visible: Vec<&Pack> = match self.showing {
            Showing::Everything => self.packs.iter().collect(),
            Showing::Pack(id) => self.packs.iter().filter(|pack| pack.id == *id).collect(),
        };

        let mut grid = div().flex().flex_wrap().gap_1().p_2();
        let mut found = 0;
        for pack in visible {
            for sticker in pack.matching(self.query) {
                found += 1;
                let chosen = Chosen {
                    pack_id: pack.id.clone(),
                    key: pack.key.clone(),
                    sticker_id: sticker.id,
                    emoji: sticker.emoji.clone(),
                    path: sticker.path.clone(),
                };
                let pick = self.on_pick.clone();

                grid = grid.child(
                    div()
                        .id(SharedString::from(format!(
                            "sticker-{}-{}",
                            hex(&pack.id),
                            sticker.id
                        )))
                        .size(px(TILE))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(kit::RADIUS))
                        .cursor_pointer()
                        .hover(|this| this.bg(theme.hover))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            pick(&chosen, window, cx)
                        })
                        .child(image::picture(&sticker.path, TILE - 12.0, TILE - 12.0)),
                );
            }
        }

        if found == 0 {
            grid = div().p_4().child(
                div()
                    .text_size(px(theme.typography.ui_size - 1.0))
                    .text_color(theme.text_muted)
                    .child("Nothing matches that."),
            );
        }

        shell(theme)
            .child(self.rail())
            .child(
                div().px_2().pt_1p5().child(
                    gpui_component::input::Input::new(self.search)
                        .appearance(false)
                        .bordered(false),
                ),
            )
            .child(div().id("sticker-grid").max_h(px(240.0)).overflow_y_scroll().child(grid))
    }

    /// The packs, as their own first sticker. A pack with nothing to draw is not
    /// published in the first place, so there is always a cover.
    fn rail(&self) -> gpui::Stateful<Div> {
        let theme = self.theme;
        let showing = self.showing.clone();

        div()
            .id("sticker-packs")
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .p_1p5()
            .border_b_1()
            .border_color(theme.border)
            .overflow_x_scroll()
            .child(tab(
                "all",
                showing == Showing::Everything,
                theme,
                div()
                    .size(px(COVER))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(15.0))
                    .child("★")
                    .into_any_element(),
                {
                    let on_pack = self.on_pack.clone();
                    move |window: &mut gpui::Window, cx: &mut gpui::App| {
                        on_pack(&Showing::Everything, window, cx)
                    }
                },
            ))
            .children(self.packs.iter().filter_map(|pack| {
                let cover = pack.cover()?;
                let selected = showing == Showing::Pack(pack.id.clone());
                let id = pack.id.clone();
                let on_pack = self.on_pack.clone();

                Some(tab(
                    format!("pack-{}", hex(&pack.id)),
                    selected,
                    theme,
                    image::picture(&cover.path, COVER, COVER).into_any_element(),
                    move |window: &mut gpui::Window, cx: &mut gpui::App| {
                        on_pack(&Showing::Pack(id.clone()), window, cx)
                    },
                ))
            }))
    }
}

fn tab(
    id: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
    face: gpui::AnyElement,
    on_click: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .flex_none()
        .p_1()
        .rounded(px(kit::RADIUS))
        .cursor_pointer()
        .when(selected, |this| this.bg(theme.active))
        .when(!selected, |this| this.hover(|this| this.bg(theme.hover)))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_click(window, cx))
        .child(face)
}

fn shell(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(kit::RADIUS))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
