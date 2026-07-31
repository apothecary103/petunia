use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{button, column, container, row, text};
use iced::{Center, Fill, Shrink, Task};
use uuid::Uuid;

use crate::config;
use crate::data::{self, State, Thread};
use crate::pane::{self, Pane};
use crate::signal;
use crate::theme;
use crate::widget::{Element, avatar, sidebar};

pub struct Main {
    state: State,
    panes: pane_grid::State<Pane>,
    focus: pane_grid::Pane,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    PaneClicked(pane_grid::Pane),
    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    SplitPane(pane_grid::Axis),
    ClosePane,
    MaximizePane,
    Buffer(pane_grid::Pane, pane::Message),
    OpenThread(Thread),
    DismissError,
}

impl Main {
    pub fn new(aci: Uuid, layout: Option<&config::Layout>) -> (Self, Vec<signal::Command>) {
        let (panes, threads) = match layout {
            Some(layout) => {
                let (configuration, threads) = restore(layout);
                (pane_grid::State::with_configuration(configuration), threads)
            }
            None => (pane_grid::State::new(Pane::empty()).0, Vec::new()),
        };
        let focus = panes
            .iter()
            .next()
            .map(|(pane, _)| *pane)
            .expect("pane grid has at least one pane");
        let commands = threads.into_iter().map(signal::Command::load).collect();

        (
            Self {
                state: State::new(aci),
                panes,
                focus,
                error: None,
            },
            commands,
        )
    }

    pub fn update(&mut self, message: Message) -> (Task<Message>, Vec<signal::Command>) {
        let commands = match message {
            Message::PaneClicked(pane) => {
                self.focus = pane;
                Vec::new()
            }
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
                Vec::new()
            }
            Message::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
                Vec::new()
            }
            Message::PaneDragged(_) => Vec::new(),
            Message::SplitPane(axis) => {
                if let Some((pane, _)) = self.panes.split(axis, self.focus, Pane::empty()) {
                    self.focus = pane;
                }
                Vec::new()
            }
            Message::ClosePane => {
                if let Some((_, sibling)) = self.panes.close(self.focus) {
                    self.focus = sibling;
                }
                Vec::new()
            }
            Message::MaximizePane => {
                if self.panes.maximized().is_some() {
                    self.panes.restore();
                } else {
                    self.panes.maximize(self.focus);
                }
                Vec::new()
            }
            Message::DismissError => {
                self.error = None;
                Vec::new()
            }
            Message::OpenThread(thread) => self.open_thread(thread),
            Message::Buffer(pane, message) => self.update_pane(pane, message),
        };

        (Task::none(), commands)
    }

    fn update_pane(&mut self, pane: pane_grid::Pane, message: pane::Message) -> Vec<signal::Command> {
        let Some(buffer) = self.panes.get_mut(pane) else {
            return Vec::new();
        };
        match buffer.update(message, &self.state) {
            pane::Action::None => Vec::new(),
            pane::Action::Command(command) => {
                self.apply_locally(&command);
                vec![command]
            }
        }
    }

    /// Reflects a command in local state before the worker answers: a sent
    /// message appears immediately, and a page request marks the thread loading
    /// so it cannot be requested twice.
    fn apply_locally(&mut self, command: &signal::Command) {
        match command {
            signal::Command::SendText {
                thread,
                body,
                timestamp,
            } => {
                let message = data::Message {
                    status: Some(data::Status::Sending),
                    ..data::Message::plain(
                        data::MessageId {
                            timestamp: *timestamp,
                            sender: self.state.aci,
                        },
                        body.clone(),
                    )
                };
                self.state.record(thread, &message);
                self.state.history_mut(thread).insert(message);
            }
            signal::Command::LoadThread { thread, .. } => {
                self.state.history_mut(thread).set_loading(true);
            }
        }
    }

    fn open_thread(&mut self, thread: Thread) -> Vec<signal::Command> {
        self.state.index.clear_unread(&thread);
        if let Some(pane) = self.panes.get_mut(self.focus) {
            *pane = Pane::chat(thread.clone());
        }
        match self.state.history(&thread) {
            Some(history) if !history.is_empty() => Vec::new(),
            _ => {
                let command = signal::Command::load(thread);
                self.apply_locally(&command);
                vec![command]
            }
        }
    }

    pub fn on_signal(&mut self, event: signal::Event) -> Vec<signal::Command> {
        match event {
            signal::Event::Contacts { contacts, groups } => {
                self.state.contacts_updated(contacts, groups);
            }
            signal::Event::Avatar { thread, bytes } => {
                self.state
                    .avatars
                    .insert(thread, iced::widget::image::Handle::from_bytes(bytes));
            }
            signal::Event::Preview { thread, message } => {
                self.state.record(&thread, &message);
            }
            signal::Event::History {
                thread,
                messages,
                more,
                older,
            } => {
                if !older
                    && let Some(last) = messages.last().cloned()
                {
                    self.state.record(&thread, &last);
                }
                let history = self.state.history_mut(&thread);
                if older {
                    history.prepend(messages, more);
                } else {
                    history.merge(messages, more);
                }
            }
            signal::Event::Message { thread, message } => self.message_received(thread, message),
            signal::Event::MessageStatus { timestamps, status } => {
                let aci = self.state.aci;
                for history in self.state.histories.values_mut() {
                    history.apply_status(&timestamps, aci, status);
                }
            }
            signal::Event::Error(error) => self.error = Some(error),
            signal::Event::Ready(_) | signal::Event::LinkUrl(_) | signal::Event::Linked { .. } => {}
        }
        Vec::new()
    }

    fn message_received(&mut self, thread: Thread, message: data::Message) {
        self.state.record(&thread, &message);

        let visible = self
            .panes
            .iter()
            .any(|(_, pane)| pane.thread() == Some(&thread));
        if message.sender() != self.state.aci && !visible {
            let mentioned = message.mentions(self.state.aci);
            self.state.index.mark_unread(&thread, mentioned);
        }

        self.state.history_mut(&thread).insert(message);
    }

    pub fn layout(&self) -> config::Layout {
        node_layout(self.panes.layout(), &self.panes)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |id, pane, _is_maximized| {
            let focused = id == self.focus;
            let heading: Element<'_, Message> = match pane.thread() {
                Some(thread) => {
                    let title = self.state.title(thread);
                    row![
                        avatar::view(&title, thread_accent(thread), 20.0, self.state.avatar(thread)),
                        text(title).size(13).font(theme::FONT_BOLD).height(Shrink),
                    ]
                    .spacing(8)
                    .align_y(Center)
                    .into()
                }
                None => text("Petunia")
                    .size(13)
                    .style(theme::text_dim)
                    .height(Shrink)
                    .into(),
            };

            let mut title_bar = pane_grid::TitleBar::new(heading).padding([8, 12]);
            if focused {
                title_bar = title_bar
                    .controls(pane_grid::Controls::new(controls()))
                    .always_show_controls();
            }

            pane_grid::Content::new(
                pane.view(&self.state)
                    .map(move |message| Message::Buffer(id, message)),
            )
            .title_bar(title_bar)
            .style(move |theme| theme::pane(theme, focused))
        })
        .on_click(Message::PaneClicked)
        .on_drag(Message::PaneDragged)
        .on_resize(8, Message::PaneResized)
        .spacing(8);

        let content = row![
            sidebar::view(&self.state).map(Message::OpenThread),
            container(grid.width(Fill).height(Fill)).padding(8),
        ];

        match &self.error {
            Some(error) => column![error_banner(error), content].into(),
            None => content.into(),
        }
    }
}

fn thread_accent(thread: &Thread) -> iced::Color {
    match thread {
        Thread::Contact(contact) => theme::accent(contact.uuid().as_bytes()),
        Thread::Group(master_key) => theme::accent(master_key),
    }
}

fn error_banner(error: &str) -> Element<'_, Message> {
    container(
        row![
            text(error).size(13).width(Fill),
            button(text("×").size(13).height(Shrink))
                .on_press(Message::DismissError)
                .padding([0, 6])
                .style(|_theme, _status| button::Style {
                    text_color: theme::colors().on_accent,
                    ..button::Style::default()
                }),
        ]
        .spacing(8),
    )
    .padding([4, 8])
    .width(Fill)
    .style(theme::error_banner)
    .into()
}

fn controls<'a>() -> Element<'a, Message> {
    let control = |label, message| {
        button(text(label).size(12).height(Shrink))
            .on_press(message)
            .padding([2, 6])
            .style(theme::pane_control)
    };

    row![
        control("-", Message::SplitPane(pane_grid::Axis::Horizontal)),
        control("|", Message::SplitPane(pane_grid::Axis::Vertical)),
        control("+", Message::MaximizePane),
        control("×", Message::ClosePane),
    ]
    .spacing(2)
    .into()
}

fn node_layout(node: &pane_grid::Node, panes: &pane_grid::State<Pane>) -> config::Layout {
    match node {
        pane_grid::Node::Split {
            axis, ratio, a, b, ..
        } => config::Layout::Split {
            axis: match axis {
                pane_grid::Axis::Horizontal => config::Axis::Horizontal,
                pane_grid::Axis::Vertical => config::Axis::Vertical,
            },
            ratio: *ratio,
            a: Box::new(node_layout(a, panes)),
            b: Box::new(node_layout(b, panes)),
        },
        pane_grid::Node::Pane(pane) => {
            config::Layout::Pane(panes.get(*pane).and_then(Pane::thread).cloned())
        }
    }
}

fn restore(layout: &config::Layout) -> (pane_grid::Configuration<Pane>, Vec<Thread>) {
    match layout {
        config::Layout::Split { axis, ratio, a, b } => {
            let (a, mut threads) = restore(a);
            let (b, more) = restore(b);
            threads.extend(more);
            (
                pane_grid::Configuration::Split {
                    axis: match axis {
                        config::Axis::Horizontal => pane_grid::Axis::Horizontal,
                        config::Axis::Vertical => pane_grid::Axis::Vertical,
                    },
                    ratio: *ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                },
                threads,
            )
        }
        config::Layout::Pane(thread) => {
            let pane = match thread {
                Some(thread) => Pane::chat(thread.clone()),
                None => Pane::empty(),
            };
            (
                pane_grid::Configuration::Pane(pane),
                thread.iter().cloned().collect(),
            )
        }
    }
}
