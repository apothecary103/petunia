use iced::widget::{column, container, qr_code, text};
use iced::{Center, Fill};

use crate::theme;
use crate::widget::Element;

pub struct Linking {
    state: State,
}

enum State {
    Connecting,
    Url(qr_code::Data),
    Failed(String),
}

impl Linking {
    pub fn new() -> Self {
        Self {
            state: State::Connecting,
        }
    }

    pub fn set_url(&mut self, url: &str) {
        self.state = match qr_code::Data::new(url) {
            Ok(data) => State::Url(data),
            Err(error) => State::Failed(format!("failed to render the provisioning code: {error}")),
        };
    }

    pub fn fail(&mut self, error: String) {
        self.state = State::Failed(error);
    }

    pub fn view<Message: 'static>(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match &self.state {
            State::Connecting => text("Connecting to Signal…")
                .size(14)
                .style(theme::text_dim)
                .into(),
            State::Url(data) => column![
                text("Link Petunia to your phone")
                    .size(18)
                    .font(theme::FONT_BOLD),
                text("Open Signal on your phone, go to Settings, Linked devices, and scan this code.")
                    .size(13)
                    .style(theme::text_dim),
                qr_code(data).cell_size(6),
            ]
            .spacing(16)
            .align_x(Center)
            .into(),
            State::Failed(error) => column![
                text("Linking failed").size(18).font(theme::FONT_BOLD),
                text(error).size(13),
                text("Restart Petunia to try again.")
                    .size(13)
                    .style(theme::text_dim),
            ]
            .spacing(8)
            .align_x(Center)
            .into(),
        };
        container(content).center(Fill).into()
    }
}
