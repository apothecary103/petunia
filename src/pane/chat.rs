use iced::widget::{column, container, text_input};

use super::Action;
use crate::data::{State, Thread};
use crate::signal;
use crate::theme;
use crate::widget::{Element, message_view};

pub struct Chat {
    pub thread: Thread,
    input: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    InputChanged(String),
    Submit,
}

impl Chat {
    pub fn new(thread: Thread) -> Self {
        Self {
            thread,
            input: String::new(),
        }
    }

    pub fn update(&mut self, message: Message, _state: &State) -> Action {
        match message {
            Message::InputChanged(input) => {
                self.input = input;
                Action::None
            }
            Message::Submit => {
                let body = self.input.trim().to_string();
                if body.is_empty() {
                    return Action::None;
                }
                self.input.clear();
                Action::Command(signal::Command::SendText {
                    thread: self.thread.clone(),
                    body,
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                })
            }
        }
    }

    pub fn view<'a>(&'a self, state: &'a State) -> Element<'a, Message> {
        let title = state.title(&self.thread);
        let history = state.history(&self.thread);

        column![
            message_view::view(history, state),
            container(
                text_input(&format!("Message {title}…"), &self.input)
                    .on_input(Message::InputChanged)
                    .on_submit(Message::Submit)
                    .size(13)
                    .padding([7, 10])
                    .style(theme::message_input),
            )
            .padding(8),
        ]
        .into()
    }
}
