use std::path::PathBuf;

use iced::keyboard::{Key, key::Named};
use iced::widget::text_editor::{self, Binding, KeyPress};
use iced::widget::text_editor as editor;
use iced::widget::{button, column, container, row, text};
use iced::{Center, Fill, Shrink, widget};

use crate::data::{MessageId, State};
use crate::theme;
use crate::widget::Element;

#[derive(Debug, Clone)]
pub enum Message {
    Edited(text_editor::Action),
    Submit,
    Newline,
    Pick,
    Remove(usize),
    Cancel,
    /// Up arrow on an empty composer, which every chat client treats as
    /// "amend what I just said".
    EditLast,
}

pub enum Action {
    None,
    Pick,
    Cancel,
    EditLast,
    Submit(Draft),
}

/// What the composer hands over on submit.
pub struct Draft {
    pub body: String,
    pub attachments: Vec<PathBuf>,
    pub replying_to: Option<MessageId>,
    pub editing: Option<MessageId>,
}

pub struct Composer {
    content: text_editor::Content,
    attachments: Vec<PathBuf>,
    replying_to: Option<MessageId>,
    editing: Option<MessageId>,
    id: widget::Id,
}

impl Composer {
    pub fn new() -> Self {
        Self {
            content: text_editor::Content::new(),
            attachments: Vec::new(),
            replying_to: None,
            editing: None,
            id: widget::Id::unique(),
        }
    }

    pub fn id(&self) -> widget::Id {
        self.id.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.content.text().trim().is_empty() && self.attachments.is_empty()
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn has_context(&self) -> bool {
        self.replying_to.is_some() || self.editing.is_some()
    }

    pub fn reply_to(&mut self, target: MessageId) {
        self.editing = None;
        self.replying_to = Some(target);
    }

    /// Loads the existing text so the edit starts from what was sent, which is
    /// what every other client does.
    pub fn edit(&mut self, target: MessageId, body: String) {
        self.replying_to = None;
        self.editing = Some(target);
        self.content = text_editor::Content::with_text(&body);
    }

    pub fn attach(&mut self, paths: Vec<PathBuf>) {
        self.attachments.extend(paths);
    }

    pub fn clear_context(&mut self) {
        self.replying_to = None;
        if self.editing.take().is_some() {
            self.content = text_editor::Content::new();
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Edited(action) => {
                self.content.perform(action);
                Action::None
            }
            Message::Newline => {
                self.content.perform(text_editor::Action::Edit(
                    text_editor::Edit::Enter,
                ));
                Action::None
            }
            Message::Pick => Action::Pick,
            Message::EditLast => Action::EditLast,
            Message::Remove(index) => {
                if index < self.attachments.len() {
                    self.attachments.remove(index);
                }
                Action::None
            }
            Message::Cancel => {
                self.clear_context();
                Action::Cancel
            }
            Message::Submit => self.submit(),
        }
    }

    pub fn submit(&mut self) -> Action {
        let body = self.content.text().trim().to_string();
        // An edit may not be emptied -- that is what deleting is for.
        if body.is_empty() && (self.attachments.is_empty() || self.editing.is_some()) {
            return Action::None;
        }
        self.content = text_editor::Content::new();

        Action::Submit(Draft {
            body,
            attachments: std::mem::take(&mut self.attachments),
            replying_to: self.replying_to.take(),
            editing: self.editing.take(),
        })
    }

    pub fn view<'a>(&'a self, placeholder: &'a str, state: &'a State) -> Element<'a, Message> {
        let colors = theme::colors();
        let mut rows = column![].spacing(4);

        if self.editing.is_some() {
            rows = rows.push(banner("Editing message", None, colors.warning));
        } else if let Some(target) = self.replying_to {
            rows = rows.push(banner(
                "Replying to",
                Some(state.sender_name(target.sender)),
                colors.accent,
            ));
        }

        if !self.attachments.is_empty() {
            rows = rows.push(strip(&self.attachments));
        }

        let empty = self.content.text().trim().is_empty();

        rows = rows.push(
            row![
                button(text("+").size(15).height(Shrink))
                    .on_press(Message::Pick)
                    .padding([2, 8])
                    .style(theme::pane_control),
                editor(&self.content)
                    .id(self.id.clone())
                    .placeholder(placeholder)
                    .on_action(Message::Edited)
                    .key_binding(move |press| binding(press, empty))
                    .size(14)
                    .padding([8, 10])
                    // Grows with the message instead of scrolling a one-line box.
                    .min_height(0.0)
                    .max_height(180.0)
                    .style(theme::composer),
            ]
            .spacing(2)
            .align_y(Center),
        );

        rows.into()
    }
}

/// Enter sends and shift+enter breaks the line, which is the convention every
/// chat client uses. Escape must be intercepted here because `text_editor` maps
/// it to `Unfocus` and consumes it, so a global listener never sees it.
fn binding(press: KeyPress, empty: bool) -> Option<Binding<Message>> {
    match press.key.as_ref() {
        Key::Named(Named::Enter) if !press.modifiers.shift() => {
            Some(Binding::Custom(Message::Submit))
        }
        Key::Named(Named::Enter) => Some(Binding::Custom(Message::Newline)),
        Key::Named(Named::Escape) => Some(Binding::Custom(Message::Cancel)),
        // Only when there is nothing to lose: otherwise up moves the cursor.
        Key::Named(Named::ArrowUp) if empty => Some(Binding::Custom(Message::EditLast)),
        _ => Binding::from_key_press(press),
    }
}

fn banner<'a>(label: &'a str, who: Option<String>, accent: iced::Color) -> Element<'a, Message> {
    let mut content = row![
        text(label)
            .size(11)
            .color(accent)
            .font(theme::FONT_BOLD)
            .height(Shrink),
    ]
    .spacing(5)
    .align_y(Center);

    if let Some(who) = who {
        content = content.push(text(who).size(11).color(accent).height(Shrink));
    }
    content = content.push(
        container(
            button(text("×").size(12).height(Shrink))
                .on_press(Message::Cancel)
                .padding([0, 5])
                .style(theme::pane_control),
        )
        .align_right(Fill),
    );

    container(content)
        .padding([2, 6])
        .width(Fill)
        .style(theme::chip)
        .into()
}

/// What is about to be sent, with a way to take any of it back out.
fn strip<'a>(attachments: &'a [PathBuf]) -> Element<'a, Message> {
    let chips = attachments.iter().enumerate().map(|(index, path)| {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());

        container(
            row![
                text(name).size(11).height(Shrink),
                button(text("×").size(11).height(Shrink))
                    .on_press(Message::Remove(index))
                    .padding(0)
                    .style(theme::pane_control),
            ]
            .spacing(5)
            .align_y(Center),
        )
        .padding([2, 6])
        .style(theme::chip)
        .into()
    });

    row(chips).spacing(4).wrap().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::Modifiers;
    use uuid::Uuid;

    fn id() -> MessageId {
        MessageId {
            timestamp: 100,
            sender: Uuid::new_v4(),
        }
    }

    fn typed(body: &str) -> Composer {
        let mut composer = Composer::new();
        composer.content = text_editor::Content::with_text(body);
        composer
    }

    fn press(key: Named, modifiers: Modifiers) -> KeyPress {
        KeyPress {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    #[test]
    fn enter_submits_and_shift_enter_does_not() {
        assert!(matches!(
            binding(press(Named::Enter, Modifiers::empty()), false),
            Some(Binding::Custom(Message::Submit))
        ));
        assert!(matches!(
            binding(press(Named::Enter, Modifiers::SHIFT), false),
            Some(Binding::Custom(Message::Newline))
        ));
    }

    /// `text_editor` maps Escape to `Unfocus` and consumes it, so without this
    /// interception a global Escape handler would never fire while typing.
    #[test]
    fn escape_is_intercepted() {
        assert!(matches!(
            binding(press(Named::Escape, Modifiers::empty()), false),
            Some(Binding::Custom(Message::Cancel))
        ));
    }

    #[test]
    fn up_edits_the_last_message_only_when_empty() {
        assert!(matches!(
            binding(press(Named::ArrowUp, Modifiers::empty()), true),
            Some(Binding::Custom(Message::EditLast))
        ));
        assert!(!matches!(
            binding(press(Named::ArrowUp, Modifiers::empty()), false),
            Some(Binding::Custom(Message::EditLast))
        ));
    }

    #[test]
    fn submitting_hands_over_the_body_and_clears() {
        let mut composer = typed("hello there");

        let Action::Submit(draft) = composer.submit() else {
            panic!("expected a submit");
        };
        assert_eq!(draft.body, "hello there");
        assert!(composer.is_empty());
    }

    #[test]
    fn whitespace_alone_does_not_submit() {
        let mut composer = typed("   \n  ");

        assert!(matches!(composer.submit(), Action::None));
    }

    #[test]
    fn an_attachment_alone_submits_with_an_empty_body() {
        let mut composer = Composer::new();
        composer.attach(vec![PathBuf::from("/tmp/cat.png")]);

        let Action::Submit(draft) = composer.submit() else {
            panic!("expected a submit");
        };
        assert!(draft.body.is_empty());
        assert_eq!(draft.attachments.len(), 1);
    }

    /// Emptying an edit would be a delete, which is a different command with
    /// different semantics on the recipient's side.
    #[test]
    fn an_empty_edit_does_not_submit() {
        let mut composer = Composer::new();
        composer.edit(id(), "original".into());
        composer.content = text_editor::Content::new();

        assert!(matches!(composer.submit(), Action::None));
    }

    #[test]
    fn editing_loads_the_existing_body() {
        let mut composer = Composer::new();

        composer.edit(id(), "original".into());

        assert_eq!(composer.content.text().trim(), "original");
        assert!(composer.is_editing());
    }

    #[test]
    fn a_reply_and_an_edit_are_mutually_exclusive() {
        let mut composer = Composer::new();

        composer.reply_to(id());
        composer.edit(id(), "text".into());
        assert!(composer.replying_to.is_none());

        composer.reply_to(id());
        assert!(composer.editing.is_none());
    }

    #[test]
    fn cancelling_an_edit_discards_the_loaded_text() {
        let mut composer = Composer::new();
        composer.edit(id(), "original".into());

        composer.clear_context();

        assert!(composer.is_empty());
        assert!(!composer.is_editing());
    }

    /// Cancelling a reply keeps what was typed: the reply target was the
    /// mistake, not the message.
    #[test]
    fn cancelling_a_reply_keeps_the_draft() {
        let mut composer = typed("my answer");
        composer.reply_to(id());

        composer.clear_context();

        assert_eq!(composer.content.text().trim(), "my answer");
        assert!(composer.replying_to.is_none());
    }

    #[test]
    fn removing_an_attachment_out_of_range_is_harmless() {
        let mut composer = Composer::new();
        composer.attach(vec![PathBuf::from("/tmp/a.png")]);

        composer.update(Message::Remove(9));

        assert_eq!(composer.attachments.len(), 1);
    }

    #[test]
    fn a_submitted_draft_carries_its_reply_target() {
        let target = id();
        let mut composer = typed("agreed");
        composer.reply_to(target);

        let Action::Submit(draft) = composer.submit() else {
            panic!("expected a submit");
        };
        assert_eq!(draft.replying_to, Some(target));
    }
}
