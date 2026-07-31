pub mod chat;

use iced::Fill;
use iced::widget::{column, container, rule, text};

pub use chat::Chat;

use crate::data::{State, Thread};
use crate::signal;
use crate::theme;
use crate::widget::Element;

/// Not `Clone`: `text_editor::Content` is not, and the composer will hold one.
pub struct Pane {
    pub buffer: Buffer,
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
}

impl Pane {
    pub fn empty() -> Self {
        Self {
            buffer: Buffer::Empty,
        }
    }

    pub fn chat(thread: Thread) -> Self {
        Self {
            buffer: Buffer::Chat(Chat::new(thread)),
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
            (Buffer::Chat(chat), Message::Chat(message)) => chat.update(message, state),
            (Buffer::Empty, _) => Action::None,
        }
    }

    pub fn view<'a>(&'a self, state: &'a State) -> Element<'a, Message> {
        let body: Element<'a, Message> = match &self.buffer {
            Buffer::Empty => container(
                text("Select a chat from the sidebar")
                    .size(13)
                    .style(theme::text_dim),
            )
            .center(Fill)
            .into(),
            Buffer::Chat(chat) => chat.view(state).map(Message::Chat),
        };
        column![rule::horizontal(1).style(theme::separator), body].into()
    }
}
