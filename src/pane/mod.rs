pub mod chat;

use iced::Task;
use iced::Fill;
use iced::widget::{column, container, rule, text};

pub use chat::Chat;

use crate::config;
use crate::data::{State, Thread};
use crate::signal;
use crate::theme;
use crate::widget::Element;

/// Not `Clone`: `text_editor::Content` is not, and the composer holds one.
pub struct Pane {
    pub buffer: Buffer,
    default_layout: config::messages::Layout,
}

pub enum Buffer {
    Empty,
    Chat(Chat),
}

#[derive(Debug, Clone)]
pub enum Message {
    Chat(chat::Message),
}

pub enum Action {
    None,
    Command(signal::Command),
    /// Focus, clipboard and the file picker are all iced-side effects, so a
    /// buffer needs a way to return one without going through the worker.
    Task(Task<Message>),
}

impl Pane {
    pub fn empty(layout: config::messages::Layout) -> Self {
        Self {
            buffer: Buffer::Empty,
            default_layout: layout,
        }
    }

    pub fn chat(thread: Thread, layout: config::messages::Layout) -> Self {
        Self {
            buffer: Buffer::Chat(Chat::new(thread, layout)),
            default_layout: layout,
        }
    }

    /// Called after the pane's thread or the contact list changes, so the
    /// composer hint names the right conversation.
    pub fn refresh(&mut self, state: &State) {
        if let Buffer::Chat(chat) = &mut self.buffer {
            chat.refresh_placeholder(state);
        }
    }

    /// Applied when the config changes, but only to panes the user has not
    /// overridden by hand -- a deliberate choice should survive a reload.
    pub fn set_default_layout(&mut self, layout: config::messages::Layout) {
        self.default_layout = layout;
        if let Buffer::Chat(chat) = &mut self.buffer {
            chat.set_default_layout(layout);
        }
    }

    pub fn action(&mut self, action: config::Action, state: &State) -> Action {
        match &mut self.buffer {
            Buffer::Chat(chat) => {
                let outcome = chat.action(action, state);
                chat.refresh_placeholder(state);
                match outcome {
                    chat::Action::None => Action::None,
                    chat::Action::Command(command) => Action::Command(command),
                    chat::Action::Task(task) => Action::Task(task.map(Message::Chat)),
                }
            }
            Buffer::Empty => Action::None,
        }
    }

    pub fn thread(&self) -> Option<&Thread> {
        match &self.buffer {
            Buffer::Chat(chat) => Some(&chat.thread),
            Buffer::Empty => None,
        }
    }

    pub fn update(&mut self, message: Message, state: &State) -> Action {
        match (&mut self.buffer, message) {
            (Buffer::Chat(chat), Message::Chat(message)) => {
                let action = chat.update(message, state);
                chat.refresh_placeholder(state);
                match action {
                    chat::Action::None => Action::None,
                    chat::Action::Command(command) => Action::Command(command),
                    chat::Action::Task(task) => Action::Task(task.map(Message::Chat)),
                }
            }
            (Buffer::Empty, _) => Action::None,
        }
    }

    /// Which layout this pane is rendering, for the title bar toggle.
    pub fn layout(&self) -> Option<config::messages::Layout> {
        match &self.buffer {
            Buffer::Chat(chat) => Some(chat.layout()),
            Buffer::Empty => None,
        }
    }

    pub fn view<'a>(
        &'a self,
        state: &'a State,
        config: &'a config::Config,
    ) -> Element<'a, Message> {
        let body: Element<'a, Message> = match &self.buffer {
            Buffer::Empty => container(
                text("Select a chat from the sidebar")
                    .size(13)
                    .style(theme::text_dim),
            )
            .center(Fill)
            .into(),
            Buffer::Chat(chat) => chat.view(state, config).map(Message::Chat),
        };
        column![rule::horizontal(1).style(theme::separator), body].into()
    }
}
