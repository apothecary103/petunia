use iced::widget::{column, container, row, scrollable, text};
use iced::{Fill, Shrink};

use super::Element;
use crate::config::{Action, Keys};
use crate::theme;

/// The keybind cheatsheet. Generated from the live bindings, so a rebound key is
/// never described wrongly here.
pub fn view<'a, M: 'a>(keys: &Keys) -> Element<'a, M> {
    let colors = theme::colors();

    let rows = keys.listing().into_iter().map(|(chord, action)| {
        row![
            container(text(chord).size(11).font(theme::FONT_BOLD).height(Shrink))
                .padding([1, 5])
                .width(130)
                .style(theme::chip),
            text(describe(action)).size(12).height(Shrink).width(Fill),
        ]
        .spacing(10)
        .align_y(iced::Center)
        .into()
    });

    container(
        column![
            text("Keyboard shortcuts")
                .size(14)
                .font(theme::FONT_BOLD)
                .height(Shrink),
            text("Edit ~/.config/petunia/config.toml to rebind")
                .size(11)
                .color(colors.dim)
                .height(Shrink),
            scrollable(column(rows).spacing(3)).height(iced::Length::Fixed(420.0)),
        ]
        .spacing(8),
    )
    .padding(14)
    .width(480)
    .style(theme::overlay)
    .into()
}

fn describe(action: Action) -> &'static str {
    match action {
        Action::QuickSwitcher => "Jump to a conversation",
        Action::FocusComposer => "Focus the message box",
        Action::NextPane => "Next pane",
        Action::PreviousPane => "Previous pane",
        Action::SplitHorizontal => "Split pane horizontally",
        Action::SplitVertical => "Split pane vertically",
        Action::ClosePane => "Close pane",
        Action::MaximizePane => "Maximise or restore pane",
        Action::ToggleLayout => "Switch between grouped and IRC layout",
        Action::ToggleSidebar => "Show or hide the sidebar",
        Action::ScrollUp => "Scroll up",
        Action::ScrollDown => "Scroll down",
        Action::ScrollToTop => "Load and scroll to the oldest loaded message",
        Action::ScrollToBottom => "Scroll to the newest message",
        Action::NextUnread => "Open the next conversation with unread messages",
        Action::MarkRead => "Mark this conversation read",
        Action::ReplyToLast => "Reply to the last message",
        Action::EditLast => "Edit your last message",
        Action::AttachFile => "Attach a file",
        Action::Cancel => "Close this, or clear the reply or edit",
        Action::Help => "Show this list",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every action must have a description, or the cheatsheet lies about what a
    /// key does. `describe` is exhaustive by construction; this pins that every
    /// bound action reaches it without panicking.
    #[test]
    fn every_default_binding_is_described() {
        for (_, action) in Keys::default().listing() {
            assert!(!describe(action).is_empty(), "{action:?} has no description");
        }
    }

    #[test]
    fn descriptions_are_unique() {
        let mut described: Vec<_> = Keys::default()
            .listing()
            .into_iter()
            .map(|(_, action)| describe(action))
            .collect();
        let before = described.len();
        described.sort_unstable();
        described.dedup();

        assert_eq!(before, described.len(), "two actions share a description");
    }
}
