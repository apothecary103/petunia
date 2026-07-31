use std::collections::{HashMap, HashSet};

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{button, column, container, image, row, text};
use iced::{Center, Fill, Shrink};
use uuid::Uuid;

use crate::config;
use crate::data::{self, Contact, Group, Thread, contact_name};
use crate::pane::{Pane, chat};
use crate::signal;
use crate::theme;
use crate::widget::{Element, avatar, sidebar};

pub struct Main {
    aci: Uuid,
    panes: pane_grid::State<Pane>,
    focus: pane_grid::Pane,
    contacts: Vec<Contact>,
    groups: Vec<Group>,
    avatars: HashMap<Thread, image::Handle>,
    previews: HashMap<Thread, data::Message>,
    unread: HashMap<Thread, u32>,
    histories: HashMap<Thread, Vec<data::Message>>,
    loaded: HashSet<Thread>,
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
    Buffer(pane_grid::Pane, chat::Message),
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
        let commands = threads.into_iter().map(signal::Command::LoadThread).collect();

        (
            Self {
                aci,
                panes,
                focus,
                contacts: Vec::new(),
                groups: Vec::new(),
                avatars: HashMap::new(),
                previews: HashMap::new(),
                unread: HashMap::new(),
                histories: HashMap::new(),
                loaded: HashSet::new(),
                error: None,
            },
            commands,
        )
    }

    pub fn update(&mut self, message: Message) -> Option<signal::Command> {
        match message {
            Message::PaneClicked(pane) => {
                self.focus = pane;
                None
            }
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
                None
            }
            Message::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.panes.drop(pane, target);
                None
            }
            Message::PaneDragged(_) => None,
            Message::SplitPane(axis) => {
                if let Some((pane, _)) = self.panes.split(axis, self.focus, Pane::empty()) {
                    self.focus = pane;
                }
                None
            }
            Message::ClosePane => {
                if let Some((_, sibling)) = self.panes.close(self.focus) {
                    self.focus = sibling;
                }
                None
            }
            Message::MaximizePane => {
                if self.panes.maximized().is_some() {
                    self.panes.restore();
                } else {
                    self.panes.maximize(self.focus);
                }
                None
            }
            Message::OpenThread(thread) => self.open_thread(thread),
            Message::DismissError => {
                self.error = None;
                None
            }
            Message::Buffer(pane, message) => {
                match self.panes.get_mut(pane)?.update(message) {
                    chat::Action::None => None,
                    chat::Action::SendText(body) => {
                        let thread = self.panes.get(pane)?.thread()?.clone();
                        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
                        let message = data::Message {
                            status: Some(data::Status::Sending),
                            ..data::Message::plain(
                                data::MessageId {
                                    timestamp,
                                    sender: self.aci,
                                },
                                body.clone(),
                            )
                        };
                        self.set_preview(&thread, &message);
                        self.histories.entry(thread.clone()).or_default().push(message);
                        Some(signal::Command::SendText {
                            thread,
                            body,
                            timestamp,
                        })
                    }
                }
            }
        }
    }

    fn open_thread(&mut self, thread: Thread) -> Option<signal::Command> {
        self.unread.remove(&thread);
        if let Some(pane) = self.panes.get_mut(self.focus) {
            *pane = Pane::chat(thread.clone());
        }
        (!self.loaded.contains(&thread)).then_some(signal::Command::LoadThread(thread))
    }

    pub fn contacts_updated(&mut self, contacts: Vec<Contact>, groups: Vec<Group>) {
        self.contacts = contacts;
        self.groups = groups;
    }

    pub fn avatar_loaded(&mut self, thread: Thread, bytes: Vec<u8>) {
        self.avatars.insert(thread, image::Handle::from_bytes(bytes));
    }

    pub fn preview_loaded(&mut self, thread: Thread, message: data::Message) {
        self.set_preview(&thread, &message);
    }

    fn set_preview(&mut self, thread: &Thread, message: &data::Message) {
        let newer = self
            .previews
            .get(thread)
            .is_none_or(|current| current.timestamp() <= message.timestamp());
        if newer {
            self.previews.insert(thread.clone(), message.clone());
        }
    }

    pub fn history_loaded(&mut self, thread: Thread, messages: Vec<data::Message>) {
        self.loaded.insert(thread.clone());
        let history = self.histories.entry(thread.clone()).or_default();
        let live = std::mem::replace(history, messages);
        for message in live {
            match history.iter_mut().find(|known| known.id == message.id) {
                Some(existing) => *existing = message,
                None => history.push(message),
            }
        }
        history.sort_by_key(|message| message.id);
        if let Some(last) = history.last().cloned() {
            self.set_preview(&thread, &last);
        }
    }

    pub fn message_received(&mut self, thread: Thread, message: data::Message) {
        self.set_preview(&thread, &message);
        let visible = self
            .panes
            .iter()
            .any(|(_, pane)| pane.thread() == Some(&thread));
        if message.sender() != self.aci && !visible {
            *self.unread.entry(thread.clone()).or_default() += 1;
        }
        self.histories.entry(thread).or_default().push(message);
    }

    pub fn message_status(&mut self, timestamps: &[u64], status: data::Status) {
        for history in self.histories.values_mut() {
            for message in history.iter_mut() {
                if message.sender() == self.aci
                    && timestamps.contains(&message.timestamp())
                    && message.status.is_none_or(|current| current < status)
                {
                    message.status = Some(status);
                }
            }
        }
    }

    pub fn show_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn layout(&self) -> config::Layout {
        node_layout(self.panes.layout(), &self.panes)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |id, pane, _is_maximized| {
            let focused = id == self.focus;
            let title = pane
                .thread()
                .map(|thread| self.thread_title(thread))
                .unwrap_or_else(|| "Petunia".into());

            let heading: Element<'_, Message> = match pane.thread() {
                Some(thread) => row![
                    avatar::view(&title, thread_accent(thread), 20.0, self.avatars.get(thread)),
                    text(title.clone())
                        .size(13)
                        .font(theme::FONT_BOLD)
                        .height(Shrink),
                ]
                .spacing(8)
                .align_y(Center)
                .into(),
                None => text(title.clone())
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
                pane.view(&self.histories, self.aci, &self.contacts, &title)
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
            sidebar::view(
                &self.contacts,
                &self.groups,
                &self.avatars,
                &self.previews,
                &self.unread,
                self.aci,
            )
            .map(Message::OpenThread),
            container(grid.width(Fill).height(Fill)).padding(8),
        ];

        match &self.error {
            Some(error) => column![error_banner(error), content].into(),
            None => content.into(),
        }
    }

    fn thread_title(&self, thread: &Thread) -> String {
        match thread {
            Thread::Contact(contact) => contact_name(&self.contacts, contact.uuid())
                .map(str::to_string)
                .unwrap_or_else(|| contact.uuid().to_string()[..8].to_string()),
            Thread::Group(master_key) => self
                .groups
                .iter()
                .find(|group| group.master_key == *master_key)
                .map(|group| group.title.clone())
                .unwrap_or_else(|| "Group".into()),
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
