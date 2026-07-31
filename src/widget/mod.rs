pub mod avatar;
pub mod message_view;
pub mod sidebar;

pub type Element<'a, Message> = iced::Element<'a, Message, iced::Theme, iced::Renderer>;
