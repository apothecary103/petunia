use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Messages {
    pub layout: Layout,
    pub density: Density,
    pub timestamps: Timestamps,
    /// Seconds. Messages from one sender closer together than this group.
    pub group_within: u64,
    pub date_separators: bool,
}

/// IRC packs one message per line; Grouped gives each run an avatar and header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    #[default]
    Grouped,
    Irc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Timestamps {
    #[default]
    Always,
    Hover,
    Never,
}

impl Default for Messages {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            density: Density::default(),
            timestamps: Timestamps::default(),
            group_within: 300,
            date_separators: true,
        }
    }
}

impl Layout {
    pub fn toggled(self) -> Self {
        match self {
            Self::Grouped => Self::Irc,
            Self::Irc => Self::Grouped,
        }
    }

    /// Names the layout the toggle would switch *to*, which is what a button
    /// label should promise.
    pub fn next_label(self) -> &'static str {
        match self.toggled() {
            Self::Grouped => "grouped",
            Self::Irc => "irc",
        }
    }
}

/// Every spacing number the message list uses, resolved in one place so the
/// widgets carry no magic constants.
#[derive(Debug, Clone, Copy)]
pub struct Spacing {
    pub between_runs: f32,
    pub within_run: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub avatar: f32,
    pub gutter: f32,
    pub body: f32,
    /// Timestamps, status marks, quotes and chips: everything that must read as
    /// secondary to the body without becoming illegible.
    pub small: f32,
    /// The edge of a sticker, which has no bubble and no intrinsic layout size.
    pub sticker: f32,
}

impl Density {
    pub fn spacing(self, layout: Layout) -> Spacing {
        match (self, layout) {
            (Self::Comfortable, Layout::Grouped) => Spacing {
                between_runs: 15.0,
                within_run: 3.0,
                padding_x: 16.0,
                padding_y: 12.0,
                avatar: 34.0,
                gutter: 46.0,
                body: 14.0,
                small: 11.0,
                sticker: 160.0,
            },
            (Self::Compact, Layout::Grouped) => Spacing {
                between_runs: 8.0,
                within_run: 1.0,
                padding_x: 12.0,
                padding_y: 8.0,
                avatar: 24.0,
                gutter: 32.0,
                body: 13.0,
                small: 10.0,
                sticker: 120.0,
            },
            (Self::Comfortable, Layout::Irc) => Spacing {
                between_runs: 5.0,
                within_run: 2.0,
                padding_x: 16.0,
                padding_y: 12.0,
                avatar: 0.0,
                gutter: 0.0,
                body: 14.0,
                small: 11.0,
                sticker: 140.0,
            },
            (Self::Compact, Layout::Irc) => Spacing {
                between_runs: 1.0,
                within_run: 0.0,
                padding_x: 12.0,
                padding_y: 8.0,
                avatar: 0.0,
                gutter: 0.0,
                body: 13.0,
                small: 10.0,
                sticker: 110.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toggle_names_where_it_goes_not_where_it_is() {
        assert_eq!(Layout::Grouped.next_label(), "irc");
        assert_eq!(Layout::Irc.next_label(), "grouped");
    }

    #[test]
    fn toggling_twice_returns_to_the_start() {
        assert_eq!(Layout::Grouped.toggled().toggled(), Layout::Grouped);
    }

    #[test]
    fn compact_is_tighter_than_comfortable_in_both_layouts() {
        for layout in [Layout::Grouped, Layout::Irc] {
            let compact = Density::Compact.spacing(layout);
            let comfortable = Density::Comfortable.spacing(layout);

            assert!(compact.between_runs <= comfortable.between_runs);
            assert!(compact.padding_y < comfortable.padding_y);
            assert!(compact.body <= comfortable.body);
            assert!(compact.small <= comfortable.small);
            assert!(compact.sticker < comfortable.sticker);
        }
    }

    /// Secondary text has to stay below the body and above unreadable, or the
    /// hierarchy inverts at one density and not the other.
    #[test]
    fn secondary_text_is_smaller_than_the_body_everywhere() {
        for density in [Density::Compact, Density::Comfortable] {
            for layout in [Layout::Grouped, Layout::Irc] {
                let spacing = density.spacing(layout);
                assert!(spacing.small < spacing.body);
                assert!(spacing.small >= 10.0);
            }
        }
    }

    /// IRC mode has no avatar gutter at all; reserving space for one would break
    /// the hanging indent it exists for.
    #[test]
    fn irc_reserves_no_avatar_gutter() {
        assert_eq!(Density::Comfortable.spacing(Layout::Irc).gutter, 0.0);
        assert_eq!(Density::Compact.spacing(Layout::Irc).gutter, 0.0);
    }
}
