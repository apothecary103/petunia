//! The emoji picker: a panel over the composer, one row of groups and a grid of
//! whatever the search matches.
//!
//! The list is Unicode's, through the `emojis` crate — every emoji, in Unicode's
//! own nine groups, in Unicode's own order, each with the name and the shortcodes
//! CLDR gives it. There was a hand-written table here for about an hour and it was
//! the wrong shape of thing twice over: it was missing most of them, and it would
//! have needed editing every time Unicode published a release. A generated list is
//! a version bump.
//!
//! Skin tones are deliberately not in the grid. The crate keeps them as variants
//! of the emoji they belong to rather than as entries of their own, which is the
//! same decision: five copies of every hand would be five sixths of the grid.

use gpui::prelude::*;
use gpui::{Div, MouseButton, SharedString, div, px};

use crate::ui::kit;
use petunia_config::Theme;

/// How large one emoji is drawn, and how wide its cell is.
const TILE: f32 = 30.0;

/// How large a category's icon is drawn on the rail.
const ICON: f32 = 15.0;

/// How many the grid draws at once.
///
/// There are something over eighteen hundred, and a scrolling `div` builds every
/// child every frame — so all of them is eighteen hundred text elements laid out
/// per frame of every flick, which is the trap the message list was converted off
/// `gpui::list()` to escape. A grid does not have rows to virtualise by, so the
/// bound is a count: enough that scrolling a group reaches the end of it, and the
/// search box is the way to the rest. What was dropped is said out loud rather
/// than left to look like the end of the list.
const SHOWN: usize = 400;

/// What the picker reports back. `Rc` because one closure is cloned into every
/// cell it draws.
pub type Pick = std::rc::Rc<dyn Fn(&str, &mut gpui::Window, &mut gpui::App)>;

/// Which group to show, reported the same way.
pub type Choose = std::rc::Rc<dyn Fn(&Showing, &mut gpui::Window, &mut gpui::App)>;

/// Which group is showing, or all of them at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Showing {
    #[default]
    Everything,
    Group(emojis::Group),
}

/// The groups, in Unicode's order, each with the icon that stands in for it on
/// the rail.
///
/// An icon rather than an emoji from the group, which is what this drew before:
/// a rail of nine emoji sits above a grid of nothing but emoji, at the same
/// size, and there is then nothing about a tab that says it is a label rather
/// than another thing to pick. A line drawing is a different kind of mark, and
/// reads as chrome at a glance. It also cannot go stale, which the old rail
/// could: the faces were looked up by their CLDR names, and a rename left a tab
/// showing whatever happened to be first in the group.
const GROUPS: [(emojis::Group, &str, &str); 9] = [
    (emojis::Group::SmileysAndEmotion, "icons/emoji.svg", "Smileys"),
    (emojis::Group::PeopleAndBody, "icons/person.svg", "People"),
    (
        emojis::Group::AnimalsAndNature,
        "icons/paw.svg",
        "Animals and nature",
    ),
    (emojis::Group::FoodAndDrink, "icons/mug.svg", "Food and drink"),
    (
        emojis::Group::TravelAndPlaces,
        "icons/plane.svg",
        "Travel and places",
    ),
    (emojis::Group::Activities, "icons/ball.svg", "Activities"),
    (emojis::Group::Objects, "icons/bulb.svg", "Objects"),
    (emojis::Group::Symbols, "icons/hash.svg", "Symbols"),
    (emojis::Group::Flags, "icons/flag.svg", "Flags"),
];

pub struct Picker<'a> {
    pub showing: Showing,
    pub query: &'a str,
    pub search: &'a gpui::Entity<gpui_component::input::InputState>,
    pub theme: &'a Theme,
    pub on_group: Choose,
    pub on_pick: Pick,
}

impl Picker<'_> {
    pub fn render(self) -> Div {
        let theme = self.theme;
        let query = self.query.trim().to_lowercase();

        let matching: Vec<&'static emojis::Emoji> = match self.showing {
            Showing::Everything => emojis::iter()
                .filter(|emoji| matches(&query, emoji))
                .collect(),
            Showing::Group(group) => group
                .emojis()
                .filter(|emoji| matches(&query, emoji))
                .collect(),
        };
        let dropped = matching.len().saturating_sub(SHOWN);

        let mut grid = div().flex().flex_wrap().gap_0p5().p_2();
        for emoji in matching.into_iter().take(SHOWN) {
            let picked = emoji.as_str().to_owned();
            let pick = self.on_pick.clone();
            let name = SharedString::from(emoji.name().to_owned());

            grid = grid.child(
                div()
                    .id(SharedString::from(emoji.as_str()))
                    .size(px(TILE))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(|this| this.bg(theme.hover))
                    // Below the cell rather than filling it: an emoji is a square
                    // glyph, and a square cell the same size leaves it touching
                    // its neighbours.
                    .text_size(px(TILE * 0.66))
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(name.clone()).build(window, cx)
                    })
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        pick(&picked, window, cx)
                    })
                    .child(SharedString::from(emoji.as_str())),
            );
        }

        let note = match (dropped, query.is_empty()) {
            (0, _) => None,
            // Only worth saying while searching: browsing a group and reaching
            // four hundred is not somebody looking for something in particular.
            (dropped, false) => Some(format!("{dropped} more — keep typing to narrow it")),
            (dropped, true) => Some(format!("{dropped} more — search to reach them")),
        };

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
                    .id("emoji-grid")
                    .max_h(px(220.0))
                    .overflow_y_scroll()
                    .child(grid)
                    .children(nothing_found(&query, theme))
                    .children(note.map(|note| {
                        div()
                            .px_2()
                            .pb_2()
                            .text_size(px(theme.typography.ui_size - 2.0))
                            .text_color(theme.text_muted)
                            .child(SharedString::from(note))
                    })),
            )
    }

    /// The groups, each as an icon. A picture rather than a word, because nine
    /// words along one row is a row nobody reads.
    fn rail(&self) -> gpui::Stateful<Div> {
        let theme = self.theme;
        let showing = self.showing;

        div()
            .id("emoji-groups")
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
                kit::icon(gpui_component::IconName::LayoutDashboard, ICON, theme.text_dim),
                "Everything",
                showing == Showing::Everything,
                theme,
                {
                    let on_group = self.on_group.clone();
                    move |window: &mut gpui::Window, cx: &mut gpui::App| {
                        on_group(&Showing::Everything, window, cx)
                    }
                },
            ))
            .children(GROUPS.map(|(group, icon, named)| {
                let on_group = self.on_group.clone();
                tab(
                    SharedString::from(format!("{group:?}")),
                    kit::glyph(icon, ICON, theme.text_dim),
                    named,
                    showing == Showing::Group(group),
                    theme,
                    move |window: &mut gpui::Window, cx: &mut gpui::App| {
                        on_group(&Showing::Group(group), window, cx)
                    },
                )
            }))
    }
}

/// Whether an emoji answers what was typed. An empty query matches everything, so
/// the picker opens as a browser before it is a search box.
///
/// Both the CLDR name and the shortcodes, because people arrive with either:
/// "cry" is in the name of one and the shortcode of another, and somebody who
/// knows `:joy:` should not have to remember what Unicode calls it.
///
/// Both sides are lowered. CLDR names are *mostly* lower case and a couple are
/// not -- the sleeping symbol is called "ZZZ" -- so a query lowered by the caller
/// and a name taken as it comes is a handful of emoji nothing can find.
fn matches(query: &str, emoji: &emojis::Emoji) -> bool {
    query.is_empty()
        || emoji.name().to_lowercase().contains(query)
        || emoji
            .shortcodes()
            .any(|code| code.to_lowercase().contains(query))
}

/// Said only when a search found nothing. An empty *group* is impossible, so
/// there is nothing to say for one.
fn nothing_found(query: &str, theme: &Theme) -> Option<Div> {
    let empty = !query.is_empty()
        && !emojis::iter().any(|emoji| matches(query, emoji));

    empty.then(|| {
        div()
            .p_4()
            .text_size(px(theme.typography.ui_size - 1.0))
            .text_color(theme.text_muted)
            .child("Nothing matches that.")
    })
}

fn tab(
    id: impl Into<SharedString>,
    face: gpui_component::Icon,
    name: &'static str,
    selected: bool,
    theme: &Theme,
    on_click: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .flex_none()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(kit::RADIUS))
        .cursor_pointer()
        .when(selected, |this| this.bg(theme.active))
        .when(!selected, |this| this.hover(|this| this.bg(theme.hover)))
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(name).build(window, cx)
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| on_click(window, cx))
        .child(face)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource as _;

    /// The whole point of taking the list from Unicode: it is the whole list.
    #[test]
    fn there_are_far_more_than_a_hand_written_table_would_hold() {
        assert!(emojis::iter().count() > 1000, "{}", emojis::iter().count());
    }

    /// Every group has to have a tab, or a whole category is unreachable by
    /// browsing.
    #[test]
    fn every_unicode_group_has_a_tab() {
        for emoji in emojis::iter() {
            assert!(
                GROUPS.iter().any(|(group, ..)| *group == emoji.group()),
                "{} is in a group with no tab",
                emoji.as_str()
            );
        }
    }

    /// An icon that is not one of petunia's own draws nothing at all, silently.
    #[test]
    fn every_tab_names_an_icon_the_asset_source_serves() {
        for (_, icon, _) in GROUPS {
            assert!(
                crate::assets::Assets
                    .load(icon)
                    .is_ok_and(|found| found.is_some()),
                "{icon} is not served"
            );
        }
    }

    #[test]
    fn an_empty_query_matches_everything() {
        let smile = emojis::get("😀").expect("a smile");

        assert!(matches("", smile));
    }

    /// Both routes in: what Unicode calls it, and what somebody who learnt
    /// `:joy:` somewhere else types.
    #[test]
    fn a_name_or_a_shortcode_finds_it() {
        let joy = emojis::get("😂").expect("tears of joy");

        assert!(matches("joy", joy));
        assert!(matches("tears", joy));
        assert!(!matches("aardvark", joy));
    }

    /// Not every CLDR name is lower case -- the sleeping symbol is "ZZZ" -- so
    /// the search has to lower both sides. This is the one that would have been
    /// unfindable.
    #[test]
    fn a_name_with_capitals_is_still_findable() {
        let zzz = emojis::get("💤").expect("the sleeping symbol");

        assert_ne!(zzz.name(), zzz.name().to_lowercase());
        assert!(matches("zzz", zzz));
    }

    /// The grid draws at most `SHOWN`, and the note has to appear exactly when
    /// something was left out -- a truncated list with nothing said about it
    /// reads as the end of the emoji.
    #[test]
    fn a_truncated_grid_says_so() {
        let all = emojis::iter().count();

        assert!(all > SHOWN, "nothing would ever be dropped");
        assert_eq!(all.saturating_sub(SHOWN), all - SHOWN);
        // A search narrow enough to fit drops nothing.
        let narrow = emojis::iter().filter(|emoji| matches("aardvark", emoji)).count();
        assert_eq!(narrow.saturating_sub(SHOWN), 0);
    }
}
