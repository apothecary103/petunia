//! The sticker picker: one tab of the panel over the composer, one row of packs
//! and a grid of whatever the search matches.

use gpui::prelude::*;
use gpui::{Div, MouseButton, SharedString, div, px};
use gpui_component::IconName;

use petunia_config::Theme;
use petunia_data::stickers::{Favourite, Pack, Sticker};
use crate::ui::{image, kit};

/// How large a sticker is drawn in the grid. Big enough to tell two cats apart,
/// small enough that a pack of sixty does not need scrolling twice.
const TILE: f32 = 68.0;
const COVER: f32 = 32.0;

/// Something the picker reports back. `Rc` because one closure is cloned into
/// every tile it draws.
pub type Pick<T> = std::rc::Rc<dyn Fn(&T, &mut gpui::Window, &mut gpui::App)>;

/// Which pack is showing, all of them at once, or the ones kept to hand.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Showing {
    #[default]
    Everything,
    Favourites,
    Pack(Vec<u8>),
}

pub struct Picker<'a> {
    pub packs: &'a [Pack],
    /// Installed packs there is nothing to draw of. What the picker says when it
    /// has nothing to show depends on it.
    pub unreadable: usize,
    pub showing: &'a Showing,
    pub query: &'a str,
    pub search: &'a gpui::Entity<gpui_component::input::InputState>,
    pub theme: &'a Theme,
    /// The stickers kept to hand, in the order they were kept.
    pub favourites: &'a [Favourite],
    pub on_pack: Pick<Showing>,
    /// Which sticker was chosen: its pack, its id, its emoji and its file.
    pub on_pick: Pick<Chosen>,
    /// The same sticker, right-clicked. What the menu offers is the
    /// conversation's business; this only says which sticker and where.
    pub on_menu: Pick<(Chosen, gpui::Point<gpui::Pixels>)>,
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
            // Two different empty pickers, and saying the wrong one is how a
            // download that failed reported itself as an account that has never
            // added a pack -- with an instruction to do the thing already done.
            let said = match self.unreadable {
                0 => "No sticker packs yet. Tap a sticker someone sent you to add its pack.",
                _ => "Your sticker packs could not be downloaded. Petunia will try again \
                      the next time it starts.",
            };
            return div().p_4().child(
                div()
                    .text_size(px(theme.typography.ui_size - 1.0))
                    .text_color(theme.text_muted)
                    .child(said),
            );
        }

        let shown = self.shown();
        let mut grid = div().flex().flex_wrap().gap_1().p_2();
        for (pack, sticker) in &shown {
            grid = grid.child(self.tile(pack, sticker));
        }

        div()
            .flex()
            .flex_col()
            .child(self.rail())
            .child(
                div().px_2().pt_1p5().child(
                    gpui_component::input::Input::new(self.search)
                        .appearance(false)
                        .bordered(false),
                ),
            )
            .child(
                div()
                    .id("sticker-grid")
                    .max_h(px(240.0))
                    .overflow_y_scroll()
                    .when(!shown.is_empty(), |this| this.child(grid))
                    .when(shown.is_empty(), |this| this.child(self.nothing())),
            )
    }

    /// What the grid draws, in the order it draws it. Favourites keep the order
    /// they were kept in; everything else keeps the packs' own.
    fn shown(&self) -> Vec<(&Pack, &Sticker)> {
        match self.showing {
            Showing::Favourites => petunia_data::stickers::favourites(self.packs, self.favourites)
                .into_iter()
                .filter(|(pack, sticker)| {
                    pack.matching(self.query)
                        .iter()
                        .any(|matched| matched.id == sticker.id)
                })
                .collect(),
            Showing::Everything => self
                .packs
                .iter()
                .flat_map(|pack| pack.matching(self.query).into_iter().map(move |s| (pack, s)))
                .collect(),
            Showing::Pack(id) => self
                .packs
                .iter()
                .filter(|pack| pack.id == *id)
                .flat_map(|pack| pack.matching(self.query).into_iter().map(move |s| (pack, s)))
                .collect(),
        }
    }

    /// What is said when the grid would be empty, which is two different things:
    /// a search that found nothing, and a favourites tab nobody has put anything
    /// in yet -- the second is an instruction, not a failure.
    fn nothing(&self) -> Div {
        let said = match (self.showing, self.query.trim().is_empty()) {
            (Showing::Favourites, true) => {
                "Nothing kept yet. Right-click a sticker to keep it here."
            }
            _ => "Nothing matches that.",
        };

        div().p_4().child(
            div()
                .text_size(px(self.theme.typography.ui_size - 1.0))
                .text_color(self.theme.text_muted)
                .child(said),
        )
    }

    fn tile(&self, pack: &Pack, sticker: &Sticker) -> gpui::Stateful<Div> {
        let theme = self.theme;
        let chosen = Chosen {
            pack_id: pack.id.clone(),
            key: pack.key.clone(),
            sticker_id: sticker.id,
            emoji: sticker.emoji.clone(),
            path: sticker.path.clone(),
        };
        let raised = chosen.clone();
        let pick = self.on_pick.clone();
        let menu = self.on_menu.clone();

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
            .on_mouse_down(MouseButton::Right, move |event: &gpui::MouseDownEvent, window, cx| {
                menu(&(raised.clone(), event.position), window, cx)
            })
            // Still, not animated. A pack is a hundred stickers and an animated
            // one is up to `MAX_FRAMES` resamples: a grid of them playing at once
            // is both unreadable and a hundredfold of the memory, which is what
            // pushed every avatar and every photograph out of the image cache the
            // moment the picker opened. What it looks like moving is what the
            // message shows.
            .child(image::picture(&sticker.path, TILE - 12.0, TILE - 12.0))
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
                mark(IconName::LayoutDashboard, theme),
                {
                    let on_pack = self.on_pack.clone();
                    move |window: &mut gpui::Window, cx: &mut gpui::App| {
                        on_pack(&Showing::Everything, window, cx)
                    }
                },
            ))
            // Always drawn, empty or not: a tab that appears only once something
            // is in it is a tab nobody finds out how to fill.
            .child(tab(
                "favourites",
                showing == Showing::Favourites,
                theme,
                mark(IconName::Heart, theme),
                {
                    let on_pack = self.on_pack.clone();
                    move |window: &mut gpui::Window, cx: &mut gpui::App| {
                        on_pack(&Showing::Favourites, window, cx)
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

/// A rail tab that stands for something other than a pack, drawn the size a
/// cover is so the row keeps one rhythm.
fn mark(icon: IconName, theme: &Theme) -> gpui::AnyElement {
    div()
        .size(px(COVER))
        .flex()
        .items_center()
        .justify_center()
        .child(kit::icon(icon, 16.0, theme.text_dim))
        .into_any_element()
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

/// Enough of a pack id to tell one element from another, which is all an id is.
fn hex(bytes: &[u8]) -> String {
    petunia_data::hex(bytes.get(..8).unwrap_or(bytes))
}
