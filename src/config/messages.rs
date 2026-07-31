use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Messages {
    pub density: Density,
    pub timestamps: Timestamps,
    /// Seconds. Messages from one sender closer together than this group.
    pub group_within: u64,
    pub date_separators: bool,
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
            density: Density::default(),
            timestamps: Timestamps::default(),
            group_within: 300,
            date_separators: true,
        }
    }
}

/// Every spacing number the message list uses, resolved in one place so the
/// views carry no magic constants.
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
    pub fn spacing(self) -> Spacing {
        match self {
            Self::Comfortable => Spacing {
                between_runs: 16.0,
                within_run: 3.0,
                padding_x: 16.0,
                padding_y: 12.0,
                avatar: 34.0,
                gutter: 48.0,
                body: 14.0,
                small: 11.0,
                sticker: 160.0,
            },
            Self::Compact => Spacing {
                between_runs: 8.0,
                within_run: 1.0,
                padding_x: 12.0,
                padding_y: 8.0,
                avatar: 24.0,
                gutter: 34.0,
                body: 13.0,
                small: 10.0,
                sticker: 120.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_is_tighter_than_comfortable() {
        let compact = Density::Compact.spacing();
        let comfortable = Density::Comfortable.spacing();

        assert!(compact.between_runs <= comfortable.between_runs);
        assert!(compact.padding_y < comfortable.padding_y);
        assert!(compact.body <= comfortable.body);
        assert!(compact.small <= comfortable.small);
        assert!(compact.sticker < comfortable.sticker);
    }

    /// Secondary text has to stay below the body and above unreadable, or the
    /// hierarchy inverts at one density and not the other.
    #[test]
    fn secondary_text_is_smaller_than_the_body_everywhere() {
        for density in [Density::Compact, Density::Comfortable] {
            let spacing = density.spacing();

            assert!(spacing.small < spacing.body);
            assert!(spacing.small >= 10.0);
        }
    }

    /// The gutter has to clear the avatar, or the run header sits on top of it.
    #[test]
    fn the_gutter_leaves_room_for_the_avatar() {
        for density in [Density::Compact, Density::Comfortable] {
            let spacing = density.spacing();

            assert!(spacing.gutter > spacing.avatar);
        }
    }
}
