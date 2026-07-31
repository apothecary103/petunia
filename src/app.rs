use iced::{Size, Subscription, Task, Theme, window};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, warn};

use crate::session::{Session, WindowSize};
use crate::screen::{self, Screen};
use crate::signal;
use crate::theme;
use crate::widget::Element;

pub struct Petunia {
    session: Session,
    screen: Screen,
    commands: Option<UnboundedSender<signal::Command>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    WindowOpened,
    WindowResized(Size),
    WindowCloseRequested,
    Signal(signal::Event),
    Main(screen::main::Message),
}

impl Petunia {
    pub fn new() -> (Self, Task<Message>) {
        let session = Session::load();
        let (_, open) = window::open(window::Settings {
            size: Size::new(session.window.width, session.window.height),
            exit_on_close_request: false,
            ..window::Settings::default()
        });

        (
            Self {
                session,
                screen: Screen::Linking(screen::Linking::new()),
                commands: None,
            },
            open.map(|_| Message::WindowOpened),
        )
    }

    pub fn title(&self, _window: window::Id) -> String {
        "Petunia".into()
    }

    pub fn theme(&self, _window: window::Id) -> Theme {
        theme::dark()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened => Task::none(),
            Message::WindowResized(size) => {
                self.session.window = WindowSize {
                    width: size.width,
                    height: size.height,
                };
                Task::none()
            }
            Message::WindowCloseRequested => {
                if let Screen::Main(main) = &self.screen {
                    self.session.layout = Some(main.layout());
                }
                self.session.save();
                iced::exit()
            }
            Message::Signal(event) => {
                self.on_signal(event);
                Task::none()
            }
            Message::Main(message) => {
                let Screen::Main(main) = &mut self.screen else {
                    return Task::none();
                };
                let (task, commands) = main.update(message);
                self.dispatch(commands);
                task.map(Message::Main)
            }
        }
    }

    fn on_signal(&mut self, event: signal::Event) {
        match event {
            signal::Event::Ready(sender) => self.commands = Some(sender),
            signal::Event::LinkUrl(url) => {
                if let Screen::Linking(linking) = &mut self.screen {
                    linking.set_url(&url);
                }
            }
            signal::Event::Linked { aci } => {
                let (main, commands) = screen::Main::new(aci, self.session.layout.as_ref());
                self.screen = Screen::Main(Box::new(main));
                self.dispatch(commands);
            }
            signal::Event::Error(error) if matches!(self.screen, Screen::Linking(_)) => {
                error!(%error, "signal error while linking");
                if let Screen::Linking(linking) = &mut self.screen {
                    linking.fail(error);
                }
            }
            event => {
                if let signal::Event::Error(error) = &event {
                    error!(%error, "signal error");
                }
                if let Screen::Main(main) = &mut self.screen {
                    let commands = main.on_signal(event);
                    self.dispatch(commands);
                }
            }
        }
    }

    fn dispatch(&self, commands: Vec<signal::Command>) {
        for command in commands {
            self.send(command);
        }
    }

    fn send(&self, command: signal::Command) {
        match &self.commands {
            Some(sender) => {
                let _ = sender.send(command);
            }
            None => warn!("signal worker not ready, dropping command"),
        }
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, Message> {
        match &self.screen {
            Screen::Linking(linking) => linking.view(),
            Screen::Main(main) => main.view().map(Message::Main),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            signal::subscription::events().map(Message::Signal),
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            window::close_requests().map(|_| Message::WindowCloseRequested),
        ])
    }
}

