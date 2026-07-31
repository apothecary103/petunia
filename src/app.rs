use std::sync::Arc;

use iced::keyboard::{Key, Modifiers};
use iced::{Size, Subscription, Task, Theme, window};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info, warn};

use crate::config::{self, Config};
use crate::screen::{self, Screen};
use crate::session::{Session, WindowSize};
use crate::signal;
use crate::theme;
use crate::widget::Element;

pub struct Petunia {
    config: Arc<Config>,
    session: Session,
    screen: Screen,
    commands: Option<UnboundedSender<signal::Command>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    WindowOpened,
    WindowResized(Size),
    WindowCloseRequested,
    WindowFocusChanged(bool),
    ConfigChanged,
    ExpireNotices,
    KeyPressed(Key, Modifiers),
    Signal(signal::Event),
    Main(screen::main::Message),
}

impl Petunia {
    pub fn new() -> (Self, Task<Message>) {
        let loaded = config::load();
        theme::install(loaded.colors);

        let session = Session::load();
        let (_, open) = window::open(window::Settings {
            size: Size::new(session.window.width, session.window.height),
            exit_on_close_request: false,
            ..window::Settings::default()
        });

        let mut petunia = Self {
            config: Arc::new(loaded.config),
            session,
            screen: Screen::Linking(screen::Linking::new()),
            commands: None,
        };
        petunia.report(loaded.errors);

        (petunia, open.map(|_| Message::WindowOpened))
    }

    pub fn title(&self, _window: window::Id) -> String {
        match &self.screen {
            // The unread count belongs in the title because a tray icon needs
            // the winit event loop, which `iced::daemon` owns and never exposes.
            Screen::Main(main) => match main.total_unread() {
                0 => "Petunia".into(),
                unread => format!("({unread}) Petunia"),
            },
            Screen::Linking(_) => "Petunia — Link a device".into(),
        }
    }

    pub fn theme(&self, _window: window::Id) -> Theme {
        theme::build()
    }

    /// Read every frame, so editing `scale` in the config takes effect without a
    /// restart -- unlike the font family, which the builder fixes before `run`.
    pub fn scale(&self, _window: window::Id) -> f32 {
        self.config.scale()
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
            Message::WindowFocusChanged(focused) => {
                if let Screen::Main(main) = &mut self.screen {
                    main.set_focused(focused);
                }
                Task::none()
            }
            Message::ConfigChanged => self.reload(),
            Message::ExpireNotices => {
                if let Screen::Main(main) = &mut self.screen {
                    main.tick();
                }
                Task::none()
            }
            Message::KeyPressed(key, modifiers) => {
                let Some(action) = self.config.keys.action(&key, modifiers) else {
                    return Task::none();
                };
                let Screen::Main(main) = &mut self.screen else {
                    return Task::none();
                };
                let (task, commands) = main.action(action);
                self.dispatch(commands);
                task.map(Message::Main)
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

    /// Re-reads the config in place. The font family cannot change — it is set on
    /// the builder before `run` — so that one is reported rather than applied.
    fn reload(&mut self) -> Task<Message> {
        let loaded = config::load();
        theme::install(loaded.colors);
        self.config = Arc::new(loaded.config);
        info!("reloaded config");

        if let Screen::Main(main) = &mut self.screen {
            main.config_changed(self.config.clone());
        }
        self.report(loaded.errors);
        Task::none()
    }

    fn report(&mut self, errors: Vec<String>) {
        if let Screen::Main(main) = &mut self.screen {
            for error in errors {
                warn!(%error, "config problem");
                main.notify(error);
            }
        } else {
            for error in errors {
                warn!(%error, "config problem");
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
                let (main, commands) = screen::Main::new(
                    aci,
                    self.config.clone(),
                    self.session.layout.as_ref(),
                );
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
            config::watch::changes().map(|()| Message::ConfigChanged),
            // Only alive while something is actually waiting to expire, so the
            // app is fully idle otherwise.
            match &self.screen {
                Screen::Main(main) if main.wants_tick() => {
                    iced::time::every(std::time::Duration::from_millis(1000))
                        .map(|_| Message::ExpireNotices)
                }
                _ => Subscription::none(),
            },
            // `listen_with` only sees events no widget consumed, so cmd+k reaches
            // the hotkey table while a bare `k` goes to the composer.
            iced::event::listen_with(|event, status, _window| match (event, status) {
                (
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        ..
                    }),
                    iced::event::Status::Ignored,
                ) => Some(Message::KeyPressed(key, modifiers)),
                (iced::Event::Window(window::Event::FileDropped(path)), _) => {
                    Some(Message::Main(screen::main::Message::FileDropped(path)))
                }
                (iced::Event::Window(window::Event::Focused), _) => {
                    Some(Message::WindowFocusChanged(true))
                }
                (iced::Event::Window(window::Event::Unfocused), _) => {
                    Some(Message::WindowFocusChanged(false))
                }
                _ => None,
            }),
        ])
    }
}
