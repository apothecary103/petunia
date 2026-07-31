use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text};
use iced::{Fill, Shrink};

use super::Element;
use crate::theme;

/// At most this many are shown; older ones are dropped rather than stacked into
/// a wall.
const MAX: usize = 3;
const EXPIRY: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Debug)]
pub struct Notice {
    pub level: Level,
    pub body: String,
    raised: Instant,
}

#[derive(Debug, Default)]
pub struct Notices {
    notices: Vec<Notice>,
}

impl Notices {
    pub fn push(&mut self, level: Level, body: String) {
        // A repeat of what is already on screen is noise, not news.
        if self.notices.iter().any(|notice| notice.body == body) {
            return;
        }
        self.notices.push(Notice {
            level,
            body,
            raised: Instant::now(),
        });
        if self.notices.len() > MAX {
            self.notices.remove(0);
        }
    }

    pub fn dismiss(&mut self, at: usize) {
        if at < self.notices.len() {
            self.notices.remove(at);
        }
    }

    /// Info and warnings fade; errors stay until dismissed, because an error the
    /// reader missed is an error they will hit again.
    pub fn expire(&mut self) {
        self.notices
            .retain(|notice| notice.level == Level::Error || notice.raised.elapsed() < EXPIRY);
    }

    pub fn is_empty(&self) -> bool {
        self.notices.is_empty()
    }

    /// Whether anything on screen still needs a tick to expire it.
    pub fn has_expiring(&self) -> bool {
        self.notices
            .iter()
            .any(|notice| notice.level != Level::Error)
    }

    pub fn view<'a, M: 'a + Clone>(&'a self, dismiss: impl Fn(usize) -> M) -> Element<'a, M> {
        let cards = self.notices.iter().enumerate().map(|(at, notice)| {
            let colors = theme::colors();
            let accent = match notice.level {
                Level::Info => colors.accent,
                Level::Warning => colors.warning,
                Level::Error => colors.danger,
            };

            container(
                row![
                    text(&notice.body).size(12).width(Fill).height(Shrink),
                    button(text("×").size(13).height(Shrink))
                        .on_press(dismiss(at))
                        .padding([0, 4])
                        .style(theme::pane_control),
                ]
                .spacing(8),
            )
            .padding([5, 8])
            .width(360)
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(theme::colors().surface)),
                border: iced::Border {
                    width: 1.0,
                    color: accent,
                    radius: iced::border::radius(6),
                },
                text_color: Some(theme::colors().text),
                ..container::Style::default()
            })
            .into()
        });

        column(cards).spacing(4).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushing_keeps_at_most_three() {
        let mut notices = Notices::default();

        for at in 0..5 {
            notices.push(Level::Info, format!("notice {at}"));
        }

        assert_eq!(notices.notices.len(), MAX);
        // The oldest went, so the newest survived.
        assert_eq!(notices.notices.last().unwrap().body, "notice 4");
    }

    /// The previous single-slot `Option<String>` silently dropped whichever error
    /// arrived first; a repeat is different from a second problem.
    #[test]
    fn a_duplicate_body_is_not_stacked() {
        let mut notices = Notices::default();

        notices.push(Level::Error, "the same problem".into());
        notices.push(Level::Error, "the same problem".into());

        assert_eq!(notices.notices.len(), 1);
    }

    #[test]
    fn errors_outlive_their_expiry() {
        let mut notices = Notices::default();
        notices.push(Level::Error, "stays".into());
        notices.notices[0].raised = Instant::now() - EXPIRY * 2;

        notices.expire();

        assert_eq!(notices.notices.len(), 1);
    }

    #[test]
    fn info_expires() {
        let mut notices = Notices::default();
        notices.push(Level::Info, "goes".into());
        notices.notices[0].raised = Instant::now() - EXPIRY * 2;

        notices.expire();

        assert!(notices.is_empty());
    }

    #[test]
    fn a_fresh_notice_survives_expiry() {
        let mut notices = Notices::default();
        notices.push(Level::Warning, "recent".into());

        notices.expire();

        assert!(!notices.is_empty());
    }

    #[test]
    fn only_expiring_levels_ask_for_a_tick() {
        let mut notices = Notices::default();
        notices.push(Level::Error, "persistent".into());
        assert!(!notices.has_expiring());

        notices.push(Level::Info, "transient".into());
        assert!(notices.has_expiring());
    }

    #[test]
    fn dismissing_out_of_range_is_harmless() {
        let mut notices = Notices::default();
        notices.push(Level::Info, "one".into());

        notices.dismiss(9);

        assert_eq!(notices.notices.len(), 1);
    }
}
