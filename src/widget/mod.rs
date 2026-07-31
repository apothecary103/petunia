pub mod avatar;
pub mod help;
pub mod message_view;
pub mod notice;
pub mod sidebar;
pub mod switcher;

pub type Element<'a, Message> = iced::Element<'a, Message, iced::Theme, iced::Renderer>;
