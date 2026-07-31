mod composer;

use iced::widget::operation;
use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset};
use iced::widget::{column, container};
use iced::{Task, widget};

use crate::config::{self, messages::Layout};
use crate::data::{MessageId, State, Thread};
use crate::signal;
use crate::widget::message_view;
use crate::widget::{Element, message_view as view};

pub use composer::Composer;

/// How far a page key moves. A screenful is not knowable from here, so this is a
/// deliberate, predictable jump.
const PAGE_SCROLL: f32 = 400.0;

/// The chat pane's own outcome type: its tasks are over its own message, so it
/// cannot use the pane-level `Action` directly.
pub enum Action {
    None,
    Command(signal::Command),
    Task(Task<Message>),
}

pub struct Chat {
    pub thread: Thread,
    composer: Composer,
    /// `None` means "follow the config"; `Some` is a deliberate per-pane
    /// override that a config reload must not stamp on.
    override_layout: Option<Layout>,
    default_layout: Layout,
    scroll: widget::Id,
    /// Spoiler segments the reader has uncovered, as (message, byte offset).
    revealed: Vec<(u64, usize)>,
    /// Held rather than formatted per frame, because `text_editor` borrows it.
    placeholder: String,
    /// When we last told the other side we were typing. Signal expects a re-send
    /// roughly every ten seconds, not one per keystroke.
    typing_sent: Option<std::time::Instant>,
}

/// How often a still-typing signal is repeated.
const TYPING_REFRESH: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum Message {
    Composer(composer::Message),
    View(message_view::Message),
    ToggleLayout,
    /// The picker resolved; `None` means it was dismissed.
    Picked(Option<Vec<std::path::PathBuf>>),
}

impl Chat {
    pub fn new(thread: Thread, layout: Layout) -> Self {
        Self {
            thread,
            composer: Composer::new(),
            override_layout: None,
            default_layout: layout,
            scroll: widget::Id::unique(),
            revealed: Vec::new(),
            placeholder: String::new(),
            typing_sent: None,
        }
    }

    /// Recomputed when the title could have changed, so the composer hint names
    /// the right conversation without reformatting on every frame.
    pub fn refresh_placeholder(&mut self, state: &State) {
        let wanted = if self.composer.is_editing() {
            "Edit message…".to_string()
        } else {
            format!("Message {}…", state.title(&self.thread))
        };
        if self.placeholder != wanted {
            self.placeholder = wanted;
        }
    }

    pub fn layout(&self) -> Layout {
        self.override_layout.unwrap_or(self.default_layout)
    }

    pub fn set_default_layout(&mut self, layout: Layout) {
        self.default_layout = layout;
    }

    pub fn update(&mut self, message: Message, state: &State) -> Action {
        match message {
            Message::ToggleLayout => {
                self.override_layout = Some(self.layout().toggled());
                Action::None
            }
            Message::Picked(None) => Action::None,
            Message::Picked(Some(paths)) => {
                self.composer.attach(paths);
                Action::Task(operation::focus(self.composer.id()))
            }
            Message::Composer(message) => {
                let typing = matches!(message, composer::Message::Edited(_));
                let outcome = self.composer.update(message);
                let action = self.composed(outcome, state);
                // An edit that produced nothing else is a keystroke, which is
                // exactly when a typing indicator is owed.
                match (typing, &action) {
                    (true, Action::None) => self.typing(),
                    _ => action,
                }
            }
            Message::View(message) => self.view_message(message, state),
        }
    }

    /// Keybinds that only make sense with a thread in front of you.
    pub fn action(&mut self, action: config::Action, state: &State) -> Action {
        match action {
            config::Action::FocusComposer => Action::Task(operation::focus(self.composer.id())),
            config::Action::ToggleLayout => {
                self.override_layout = Some(self.layout().toggled());
                Action::None
            }
            config::Action::AttachFile => self.pick(),
            config::Action::ScrollUp => Action::Task(operation::scroll_by(
                self.scroll.clone(),
                AbsoluteOffset {
                    x: 0.0,
                    y: -PAGE_SCROLL,
                },
            )),
            config::Action::ScrollDown => Action::Task(operation::scroll_by(
                self.scroll.clone(),
                AbsoluteOffset {
                    x: 0.0,
                    y: PAGE_SCROLL,
                },
            )),
            config::Action::ScrollToTop => Action::Task(operation::snap_to(
                self.scroll.clone(),
                RelativeOffset::START,
            )),
            config::Action::ScrollToBottom => {
                Action::Task(operation::snap_to_end(self.scroll.clone()))
            }
            config::Action::ReplyToLast => match self.last_message(state, false) {
                Some(id) => {
                    self.composer.reply_to(id);
                    Action::Task(operation::focus(self.composer.id()))
                }
                None => Action::None,
            },
            config::Action::EditLast => self.edit_last(state),
            config::Action::Cancel => {
                self.composer.clear_context();
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Rate limited, and silent once the composer is empty again.
    fn typing(&mut self) -> Action {
        if self.composer.is_empty() {
            return self.stop_typing();
        }
        let fresh = self
            .typing_sent
            .is_none_or(|last| last.elapsed() >= TYPING_REFRESH);
        if !fresh {
            return Action::None;
        }
        self.typing_sent = Some(std::time::Instant::now());
        Action::Command(signal::Command::Typing {
            thread: self.thread.clone(),
            started: true,
        })
    }

    fn stop_typing(&mut self) -> Action {
        if self.typing_sent.take().is_none() {
            return Action::None;
        }
        Action::Command(signal::Command::Typing {
            thread: self.thread.clone(),
            started: false,
        })
    }

    fn composed(&mut self, outcome: composer::Action, state: &State) -> Action {
        match outcome {
            composer::Action::None | composer::Action::Cancel => Action::None,
            composer::Action::Pick => self.pick(),
            composer::Action::EditLast => self.edit_last(state),
            composer::Action::Submit(draft) => {
                // Submitting ends the typing state; the message itself is the
                // update, so no "stopped" needs to go out.
                self.typing_sent = None;
                self.send(draft, state)
            }
        }
    }

    fn pick(&self) -> Action {
        Action::Task(
            Task::future(async {
                rfd::AsyncFileDialog::new()
                    .set_title("Attach files")
                    .pick_files()
                    .await
                    .map(|files| {
                        files
                            .into_iter()
                            .map(|file| file.path().to_path_buf())
                            .collect()
                    })
            })
            .map(Message::Picked),
        )
    }

    fn edit_last(&mut self, state: &State) -> Action {
        // Only our own: Signal has no way to edit someone else's message.
        let Some(id) = self.last_message(state, true) else {
            return Action::None;
        };
        let body = self.body_of(state, &id);
        self.composer.edit(id, body);
        Action::Task(operation::focus(self.composer.id()))
    }

    fn body_of(&self, state: &State, id: &MessageId) -> String {
        state
            .history(&self.thread)
            .and_then(|history| history.find(id))
            .and_then(|message| message.text())
            .unwrap_or_default()
            .to_string()
    }

    fn last_message(&self, state: &State, mine_only: bool) -> Option<MessageId> {
        state
            .history(&self.thread)?
            .messages()
            .iter()
            .rev()
            .find(|message| {
                message.is_addressable() && (!mine_only || message.sender() == state.aci)
            })
            .map(|message| message.id)
    }

    fn send(&mut self, draft: composer::Draft, state: &State) -> Action {
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;

        // An edit replaces a message rather than adding one, so it carries
        // neither attachments nor a quote.
        if let Some(target) = draft.editing {
            return Action::Command(signal::Command::EditMessage {
                thread: self.thread.clone(),
                target: target.timestamp,
                body: draft.body,
                timestamp,
            });
        }

        let quote = draft
            .replying_to
            .and_then(|id| quoted(state, &self.thread, id));

        if draft.attachments.is_empty() {
            return Action::Command(signal::Command::SendText {
                thread: self.thread.clone(),
                body: draft.body,
                quote,
                timestamp,
            });
        }
        Action::Command(signal::Command::SendAttachments {
            thread: self.thread.clone(),
            body: draft.body,
            paths: draft.attachments,
            quote,
            timestamp,
        })
    }

    fn view_message(&mut self, message: message_view::Message, state: &State) -> Action {
        match message {
            message_view::Message::LoadOlder => {
                let Some(history) = state.history(&self.thread) else {
                    return Action::None;
                };
                if history.is_loading() || !history.has_more() {
                    return Action::None;
                }
                Action::Command(signal::Command::LoadThread {
                    thread: self.thread.clone(),
                    before: history.oldest(),
                })
            }
            message_view::Message::Download(timestamp, id) => {
                Action::Command(signal::Command::DownloadAttachment {
                    thread: self.thread.clone(),
                    timestamp,
                    id,
                })
            }
            message_view::Message::OpenAttachment(path) => {
                Action::Task(Task::future(open(path)).discard())
            }
            message_view::Message::React(target, emoji, remove) => {
                Action::Command(signal::Command::React {
                    thread: self.thread.clone(),
                    target,
                    emoji,
                    remove,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                })
            }
            message_view::Message::Reply(target) => {
                self.composer.reply_to(target);
                Action::Task(operation::focus(self.composer.id()))
            }
            message_view::Message::Edit(target) => {
                let body = self.body_of(state, &target);
                self.composer.edit(target, body);
                Action::Task(operation::focus(self.composer.id()))
            }
            message_view::Message::Delete(target) => {
                Action::Command(signal::Command::DeleteMessage {
                    thread: self.thread.clone(),
                    target: target.timestamp,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                })
            }
            message_view::Message::Copy(body) => Action::Task(iced::clipboard::write(body)),
            message_view::Message::Link(view::Link::Url(url)) => {
                Action::Task(Task::future(open(std::path::PathBuf::from(url))).discard())
            }
            message_view::Message::Link(view::Link::Reveal(id, offset)) => {
                // Keyed by both: the same offset means different things in
                // different messages.
                let key = (id.timestamp, offset);
                if !self.revealed.contains(&key) {
                    self.revealed.push(key);
                }
                Action::None
            }
        }
    }

    pub fn view<'a>(
        &'a self,
        state: &'a State,
        config: &'a config::Config,
    ) -> Element<'a, Message> {
        let context = message_view::Context {
            state,
            messages: &config.messages,
            layout: self.layout(),
            image_max_width: config.media.image_max_width,
            image_max_height: config.media.image_max_height,
            revealed: &self.revealed,
        };

        let mut rows = column![
            message_view::view(state.history(&self.thread), context, self.scroll.clone())
                .map(Message::View),
        ];

        let typing = state.typing(&self.thread);
        if !typing.is_empty() {
            rows = rows.push(
                container(
                    iced::widget::text(typing_label(&typing, state))
                        .size(12)
                        .style(crate::theme::text_dim)
                        .height(iced::Shrink),
                )
                .padding([0, 14]),
            );
        }

        rows.push(
            container(self.composer.view(&self.placeholder, state).map(Message::Composer))
                .padding(8),
        )
        .into()
    }
}

fn typing_label(typing: &[uuid::Uuid], state: &State) -> String {
    let names: Vec<String> = typing
        .iter()
        .take(3)
        .map(|who| state.sender_name(*who))
        .collect();

    match names.len() {
        0 => String::new(),
        1 => format!("{} is typing…", names[0]),
        _ => format!("{} are typing…", names.join(", ")),
    }
}

/// A snapshot of what a reply answers, because the recipient may not have the
/// original message.
fn quoted(state: &State, thread: &Thread, id: MessageId) -> Option<signal::Quoted> {
    let message = state.history(thread)?.find(&id)?;
    Some(signal::Quoted {
        id,
        body: message.text().unwrap_or_default().to_string(),
        ranges: message.ranges().to_vec(),
    })
}

/// Handing a file or a URL to the OS is the whole story for media petunia does
/// not render itself.
async fn open(path: std::path::PathBuf) {
    let result = tokio::task::spawn_blocking(move || open::that_detached(path)).await;
    if let Err(error) = result {
        tracing::warn!(%error, "failed to hand off to the system opener");
    }
}
