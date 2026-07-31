use std::sync::Arc;

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{button, container, opaque, row, stack, text};
use iced::{Center, Fill, Shrink, Task};
use uuid::Uuid;

use crate::config::{Action, Config};
use crate::data::{self, Fragment, History, State, Thread};
use crate::pane::{self, Pane};
use crate::session;
use crate::signal;
use crate::theme;
use crate::widget::notice::{self, Notices};
use crate::widget::switcher::{self, Switcher};
use crate::widget::{Element, avatar, help, sidebar};

pub struct Main {
    config: Arc<Config>,
    state: State,
    panes: pane_grid::State<Pane>,
    focus: pane_grid::Pane,
    notices: Notices,
    overlay: Option<Overlay>,
    sidebar: bool,
    focused: bool,
}

/// Only one of these is ever meaningful at a time, which is what makes Escape
/// handling a single line rather than a priority list.
enum Overlay {
    Switcher(Switcher),
    Help,
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
    FileDropped(std::path::PathBuf),
    Switcher(switcher::Message),
    DismissNotice(usize),
    ExpireNotices,
    CloseOverlay,
}

impl Main {
    pub fn new(
        aci: Uuid,
        config: Arc<Config>,
        layout: Option<&session::Layout>,
    ) -> (Self, Vec<signal::Command>) {
        let (panes, threads) = match layout {
            Some(layout) => {
                let (configuration, threads) = restore(layout, config.messages.layout);
                (pane_grid::State::with_configuration(configuration), threads)
            }
            None => (
                pane_grid::State::new(Pane::empty(config.messages.layout)).0,
                Vec::new(),
            ),
        };
        let focus = panes
            .iter()
            .next()
            .map(|(pane, _)| *pane)
            .expect("pane grid has at least one pane");
        let commands = threads.into_iter().map(signal::Command::load).collect();

        (
            Self {
                config,
                state: State::new(aci),
                panes,
                focus,
                notices: Notices::default(),
                overlay: None,
                sidebar: true,
                focused: true,
            },
            commands,
        )
    }

    pub fn total_unread(&self) -> u32 {
        self.state.index.total_unread()
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn config_changed(&mut self, config: Arc<Config>) {
        self.config = config;
        for (_, pane) in self.panes.iter_mut() {
            pane.set_default_layout(self.config.messages.layout);
        }
    }

    /// One tick drives both, since both are "something on screen ages out".
    pub fn tick(&mut self) {
        self.notices.expire();
        self.state.expire_typing();
    }

    pub fn notify(&mut self, body: String) {
        self.notices.push(notice::Level::Warning, body);
    }

    /// Whether anything on screen needs a periodic tick. Kept narrow so the app
    /// is idle when nothing is animating.
    pub fn wants_tick(&self) -> bool {
        self.notices.has_expiring() || self.state.anyone_typing()
    }

    pub fn update(&mut self, message: Message) -> (Task<Message>, Vec<signal::Command>) {
        if let Message::Buffer(pane, message) = message {
            return self.update_pane(pane, message);
        }
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
                self.split(axis);
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
            Message::DismissNotice(at) => {
                self.notices.dismiss(at);
                Vec::new()
            }
            Message::ExpireNotices => {
                self.notices.expire();
                Vec::new()
            }
            Message::CloseOverlay => {
                self.overlay = None;
                Vec::new()
            }
            Message::Switcher(message) => return self.switcher(message),
            Message::OpenThread(thread) => self.open_thread(thread),
            Message::FileDropped(path) => self.attach(path),
            Message::Buffer(..) => unreachable!("handled above"),
        };

        (Task::none(), commands)
    }

    fn update_pane(
        &mut self,
        pane: pane_grid::Pane,
        message: pane::Message,
    ) -> (Task<Message>, Vec<signal::Command>) {
        let Some(buffer) = self.panes.get_mut(pane) else {
            return (Task::none(), Vec::new());
        };
        match buffer.update(message, &self.state) {
            pane::Action::None => (Task::none(), Vec::new()),
            pane::Action::Task(task) => (
                task.map(move |message| Message::Buffer(pane, message)),
                Vec::new(),
            ),
            pane::Action::Command(command) => {
                self.apply_locally(&command);
                (Task::none(), vec![command])
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
                quote,
                timestamp,
            } => self.echo(thread, body, Vec::new(), quote, *timestamp),
            signal::Command::SendAttachments {
                thread,
                body,
                paths,
                quote,
                timestamp,
            } => {
                let attachments = paths.iter().cloned().map(local_attachment).collect();
                self.echo(thread, body, attachments, quote, *timestamp);
            }
            signal::Command::React {
                thread,
                target,
                emoji,
                remove,
                timestamp,
            } => {
                let reaction = data::Reaction {
                    author: self.state.aci,
                    emoji: emoji.clone(),
                    timestamp: *timestamp,
                };
                self.state
                    .history_mut(thread)
                    .apply_reaction(target, reaction, *remove);
            }
            signal::Command::DeleteMessage { thread, target, .. } => {
                let id = data::MessageId {
                    timestamp: *target,
                    sender: self.state.aci,
                };
                self.state.history_mut(thread).apply_delete(&id);
                self.refresh_preview(thread);
            }
            signal::Command::EditMessage {
                thread,
                target,
                body,
                timestamp,
            } => {
                let id = data::MessageId {
                    timestamp: *target,
                    sender: self.state.aci,
                };
                let edit = data::Message::plain(id, body.clone());
                self.state
                    .history_mut(thread)
                    .apply_edit(&id, edit, *timestamp);
                self.refresh_preview(thread);
            }
            signal::Command::LoadThread { thread, .. } => {
                self.state.history_mut(thread).set_loading(true);
            }
            // Neither changes anything the UI is showing.
            signal::Command::MarkRead { .. } | signal::Command::Typing { .. } => {}
            signal::Command::DownloadAttachment { thread, id, .. } => {
                self.state
                    .history_mut(thread)
                    .set_blob(id, data::attachment::Blob::Downloading(0.0));
            }
        }
    }

    fn switcher(&mut self, message: switcher::Message) -> (Task<Message>, Vec<signal::Command>) {
        let Some(Overlay::Switcher(switcher)) = &mut self.overlay else {
            return (Task::none(), Vec::new());
        };
        match message {
            switcher::Message::Query(query) => {
                switcher.query(query);
                (Task::none(), Vec::new())
            }
            switcher::Message::Submit => {
                let selected = switcher.selection(&self.state.index);
                self.overlay = None;
                match selected {
                    Some(thread) => (Task::none(), self.open_thread(thread)),
                    None => (Task::none(), Vec::new()),
                }
            }
        }
    }

    /// One entry point for every keybind. Returning the same pair as `update`
    /// keeps `app.rs` from needing to know which actions talk to the worker.
    pub fn action(&mut self, action: Action) -> (Task<Message>, Vec<signal::Command>) {
        // An overlay owns the keyboard while it is up.
        if self.overlay.is_some() {
            return self.overlay_action(action);
        }

        match action {
            Action::QuickSwitcher => {
                let switcher = Switcher::new();
                let focus = iced::widget::operation::focus(switcher.input());
                self.overlay = Some(Overlay::Switcher(switcher));
                (focus, Vec::new())
            }
            Action::Help => {
                self.overlay = Some(Overlay::Help);
                (Task::none(), Vec::new())
            }
            Action::ToggleSidebar => {
                self.sidebar = !self.sidebar;
                (Task::none(), Vec::new())
            }
            Action::SplitVertical => {
                self.split(pane_grid::Axis::Vertical);
                (Task::none(), Vec::new())
            }
            Action::SplitHorizontal => {
                self.split(pane_grid::Axis::Horizontal);
                (Task::none(), Vec::new())
            }
            Action::ClosePane => {
                if let Some((_, sibling)) = self.panes.close(self.focus) {
                    self.focus = sibling;
                }
                (Task::none(), Vec::new())
            }
            Action::MaximizePane => {
                if self.panes.maximized().is_some() {
                    self.panes.restore();
                } else {
                    self.panes.maximize(self.focus);
                }
                (Task::none(), Vec::new())
            }
            Action::NextPane => {
                self.cycle_pane(true);
                (Task::none(), Vec::new())
            }
            Action::PreviousPane => {
                self.cycle_pane(false);
                (Task::none(), Vec::new())
            }
            Action::NextUnread => {
                let from = self.focused_thread();
                match self.state.index.next_unread(from.as_ref()).cloned() {
                    Some(thread) => (Task::none(), self.open_thread(thread)),
                    None => (Task::none(), Vec::new()),
                }
            }
            Action::MarkRead => {
                let Some(thread) = self.focused_thread() else {
                    return (Task::none(), Vec::new());
                };
                let command = self.read_receipts(&thread);
                self.state.index.clear_unread(&thread);
                self.state.history_mut(&thread).mark_unread_from(None);
                (Task::none(), vec![command])
            }
            // Everything else is the focused pane's business.
            Action::FocusComposer
            | Action::ToggleLayout
            | Action::ScrollUp
            | Action::ScrollDown
            | Action::ScrollToTop
            | Action::ScrollToBottom
            | Action::ReplyToLast
            | Action::EditLast
            | Action::AttachFile
            | Action::Cancel => self.pane_action(action),
        }
    }

    /// While an overlay is up the only meaningful keys are the ones that move
    /// through it or close it.
    fn overlay_action(&mut self, action: Action) -> (Task<Message>, Vec<signal::Command>) {
        match (&mut self.overlay, action) {
            (_, Action::Cancel) | (Some(Overlay::Help), Action::Help) => {
                self.overlay = None;
            }
            (Some(Overlay::Switcher(switcher)), Action::ScrollDown | Action::NextUnread) => {
                let total = switcher.count(&self.state.index);
                switcher.move_by(1, total);
            }
            (Some(Overlay::Switcher(switcher)), Action::ScrollUp) => {
                let total = switcher.count(&self.state.index);
                switcher.move_by(-1, total);
            }
            _ => {}
        }
        (Task::none(), Vec::new())
    }

    fn pane_action(&mut self, action: Action) -> (Task<Message>, Vec<signal::Command>) {
        let pane = self.focus;
        let Some(buffer) = self.panes.get_mut(pane) else {
            return (Task::none(), Vec::new());
        };
        match buffer.action(action, &self.state) {
            pane::Action::None => (Task::none(), Vec::new()),
            pane::Action::Task(task) => (
                task.map(move |message| Message::Buffer(pane, message)),
                Vec::new(),
            ),
            pane::Action::Command(command) => {
                self.apply_locally(&command);
                (Task::none(), vec![command])
            }
        }
    }

    fn split(&mut self, axis: pane_grid::Axis) {
        if let Some((pane, _)) = self.panes.split(axis, self.focus, self.new_pane()) {
            self.focus = pane;
        }
    }

    fn new_pane(&self) -> Pane {
        Pane::empty(self.config.messages.layout)
    }

    /// Ordered by the grid's own traversal, so the cycle follows what the eye
    /// sees rather than insertion order.
    fn cycle_pane(&mut self, forward: bool) {
        let panes: Vec<_> = self.panes.iter().map(|(pane, _)| *pane).collect();
        if panes.len() < 2 {
            return;
        }
        let at = panes.iter().position(|pane| *pane == self.focus).unwrap_or(0);
        let next = if forward {
            (at + 1) % panes.len()
        } else {
            (at + panes.len() - 1) % panes.len()
        };
        self.focus = panes[next];
    }

    fn focused_thread(&self) -> Option<Thread> {
        self.panes.get(self.focus).and_then(Pane::thread).cloned()
    }

    /// Shows a send before the worker answers, so the timeline never lags a
    /// keystroke behind.
    fn echo(
        &mut self,
        thread: &Thread,
        body: &str,
        attachments: Vec<data::attachment::Attachment>,
        quote: &Option<signal::Quoted>,
        timestamp: u64,
    ) {
        let message = data::Message {
            status: Some(data::Status::Sending),
            attachments,
            quote: quote.as_ref().map(|quoted| {
                Box::new(data::message::Quote {
                    id: quoted.id,
                    body: quoted.body.clone(),
                    ranges: quoted.ranges.clone(),
                    thumbnail: None,
                })
            }),
            ..data::Message::plain(
                data::MessageId {
                    timestamp,
                    sender: self.state.aci,
                },
                body.to_string(),
            )
        };
        self.state.record(thread, &message);
        self.state.history_mut(thread).insert(message);
    }

    /// A drop carries no cursor position, so it goes to the focused pane.
    fn attach(&mut self, path: std::path::PathBuf) -> Vec<signal::Command> {
        let Some(thread) = self.panes.get(self.focus).and_then(Pane::thread).cloned() else {
            return Vec::new();
        };
        let command = signal::Command::SendAttachments {
            thread,
            body: String::new(),
            paths: vec![path],
            quote: None,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        };
        self.apply_locally(&command);
        vec![command]
    }

    fn open_thread(&mut self, thread: Thread) -> Vec<signal::Command> {
        // Pinned before the count is cleared, so the divider marks where the
        // reader left off and then stays put.
        let unread = self.state.index.unread(&thread) as usize;
        if unread > 0 {
            let first = self.state.history(&thread).and_then(|history| {
                let messages = history.messages();
                messages
                    .len()
                    .checked_sub(unread)
                    .and_then(|at| messages.get(at))
                    .map(|message| message.timestamp())
            });
            self.state.history_mut(&thread).mark_unread_from(first);
        }
        self.state.index.clear_unread(&thread);
        if let Some(pane) = self.panes.get_mut(self.focus) {
            *pane = Pane::chat(thread.clone(), self.config.messages.layout);
            pane.refresh(&self.state);
        }

        let mut commands = Vec::new();
        if unread > 0 {
            commands.push(self.read_receipts(&thread));
        }
        match self.state.history(&thread) {
            Some(history) if !history.is_empty() => {}
            _ => {
                let command = signal::Command::load(thread);
                self.apply_locally(&command);
                commands.push(command);
            }
        }
        commands
    }

    fn read_receipts(&self, thread: &Thread) -> signal::Command {
        signal::Command::MarkRead {
            thread: thread.clone(),
            messages: self.state.unread_receipts(thread),
        }
    }

    pub fn on_signal(&mut self, event: signal::Event) -> Vec<signal::Command> {
        match event {
            signal::Event::Contacts { contacts, groups } => {
                self.state.contacts_updated(contacts, groups);
                // Names have just arrived, so every composer hint is stale.
                let state = &self.state;
                for (_, pane) in self.panes.iter_mut() {
                    pane.refresh(state);
                }
            }
            signal::Event::Profile { uuid, name } => {
                self.state.set_profile(uuid, name);
                let state = &self.state;
                for (_, pane) in self.panes.iter_mut() {
                    pane.refresh(state);
                }
            }
            signal::Event::Connection(connection) => {
                self.state.connection = connection;
            }
            signal::Event::Avatar { thread, path } => {
                self.state
                    .avatars
                    .insert(thread, iced::widget::image::Handle::from_path(path));
            }
            signal::Event::Attachment { thread, id, blob } => {
                self.state.history_mut(&thread).set_blob(&id, blob);
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
            signal::Event::Fragment {
                thread,
                fragment,
                order,
            } => self.fragment_received(thread, fragment, order),
            signal::Event::Typing {
                thread,
                sender,
                started,
            } => {
                self.state.set_typing(&thread, sender, started);
            }
            signal::Event::MessageStatus { timestamps, status } => {
                let aci = self.state.aci;
                for history in self.state.histories.values_mut() {
                    history.apply_status(&timestamps, aci, status);
                }
            }
            signal::Event::Error(error) => {
                self.notices.push(notice::Level::Error, error);
            }
            signal::Event::Ready(_) | signal::Event::LinkUrl(_) | signal::Event::Linked { .. } => {}
        }
        Vec::new()
    }

    fn fragment_received(&mut self, thread: Thread, fragment: Fragment, order: u64) {
        match fragment {
            Fragment::Message(message) => self.message_received(thread, message),
            Fragment::Edit { target, message } => {
                self.state
                    .history_mut(&thread)
                    .apply_edit(&target, message, order);
                self.refresh_preview(&thread);
            }
            Fragment::Reaction {
                target,
                reaction,
                remove,
            } => {
                self.state
                    .history_mut(&thread)
                    .apply_reaction(&target, reaction, remove);
            }
            Fragment::Delete { target } => {
                self.state.history_mut(&thread).apply_delete(&target);
                self.refresh_preview(&thread);
            }
            Fragment::Ignored => {}
        }
    }

    /// An edit or a delete changes what the sidebar should say, but only if it
    /// hit the newest message in the thread.
    fn refresh_preview(&mut self, thread: &Thread) {
        if let Some(last) = self.state.history(thread).and_then(History::last).cloned() {
            self.state.record(thread, &last);
        }
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

    pub fn layout(&self) -> session::Layout {
        node_layout(self.panes.layout(), &self.panes)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |id, pane, _is_maximized| {
            let focused = id == self.focus;
            let heading: Element<'_, Message> = match pane.thread() {
                Some(thread) => {
                    let title = self.state.title(thread);
                    row![
                        avatar::view(&title, thread_accent(thread), 24.0, self.state.avatar(thread)),
                        text(title).size(14).font(theme::FONT_BOLD).height(Shrink),
                    ]
                    .spacing(8)
                    .align_y(Center)
                    .into()
                }
                None => text("Petunia")
                    .size(14)
                    .style(theme::text_dim)
                    .height(Shrink)
                    .into(),
            };

            let mut title_bar = pane_grid::TitleBar::new(heading).padding([8, 12]);
            if focused {
                title_bar = title_bar
                    .controls(pane_grid::Controls::new(controls(id, pane.layout())))
                    .always_show_controls();
            }

            pane_grid::Content::new(
                pane.view(&self.state, &self.config)
                    .map(move |message| Message::Buffer(id, message)),
            )
            .title_bar(title_bar)
            .style(move |theme| theme::pane(theme, focused))
        })
        .on_click(Message::PaneClicked)
        .on_drag(Message::PaneDragged)
        .on_resize(8, Message::PaneResized)
        .spacing(8);

        let mut content = row![].height(Fill);
        if self.sidebar {
            content = content.push(
                sidebar::view(&self.state, &self.config.sidebar, self.focused_thread())
                    .map(Message::OpenThread),
            );
        }
        content = content.push(container(grid.width(Fill).height(Fill)).padding(8));

        let mut layers: Vec<Element<'_, Message>> = vec![content.into()];

        if !self.notices.is_empty() {
            layers.push(
                container(self.notices.view(Message::DismissNotice))
                    .align_right(Fill)
                    .align_bottom(Fill)
                    .padding(12)
                    .into(),
            );
        }

        // `opaque` keeps clicks from falling through to the grid behind, and the
        // scrim gives the overlay somewhere to be dismissed from.
        if let Some(overlay) = &self.overlay {
            let panel: Element<'_, Message> = match overlay {
                Overlay::Switcher(switcher) => {
                    switcher.view(&self.state).map(Message::Switcher)
                }
                Overlay::Help => help::view(&self.config.keys),
            };
            layers.push(opaque(
                iced::widget::mouse_area(
                    container(panel)
                        .center_x(Fill)
                        .align_top(Fill)
                        .padding(iced::Padding::default().top(80))
                        .style(|_theme| container::Style {
                            background: Some(iced::Background::Color(iced::Color {
                                a: 0.55,
                                ..theme::colors().background
                            })),
                            ..container::Style::default()
                        }),
                )
                .on_press(Message::CloseOverlay),
            ));
        }

        stack(layers).into()
    }
}

/// The echo shown while the upload runs. Sizing it needs a stat, which is local
/// and cheap; a file that has since vanished simply shows as empty and the send
/// then fails.
fn local_attachment(path: std::path::PathBuf) -> data::attachment::Attachment {
    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    data::attachment::from_path(path, size)
}

fn thread_accent(thread: &Thread) -> iced::Color {
    match thread {
        Thread::Contact(contact) => theme::accent(contact.uuid().as_bytes()),
        Thread::Group(master_key) => theme::accent(master_key),
    }
}

fn controls<'a>(
    pane: pane_grid::Pane,
    layout: Option<crate::config::messages::Layout>,
) -> Element<'a, Message> {
    let control = |label: &'a str, message| {
        button(text(label).size(12).height(Shrink))
            .on_press(message)
            .padding([2, 6])
            .style(theme::pane_control)
    };

    let mut controls = row![].spacing(2);
    if let Some(layout) = layout {
        controls = controls.push(control(
            layout.next_label(),
            Message::Buffer(pane, pane::Message::Chat(pane::chat::Message::ToggleLayout)),
        ));
    }
    controls
        .push(control("-", Message::SplitPane(pane_grid::Axis::Horizontal)))
        .push(control("|", Message::SplitPane(pane_grid::Axis::Vertical)))
        .push(control("+", Message::MaximizePane))
        .push(control("×", Message::ClosePane))
        .into()
}

fn node_layout(node: &pane_grid::Node, panes: &pane_grid::State<Pane>) -> session::Layout {
    match node {
        pane_grid::Node::Split {
            axis, ratio, a, b, ..
        } => session::Layout::Split {
            axis: match axis {
                pane_grid::Axis::Horizontal => session::Axis::Horizontal,
                pane_grid::Axis::Vertical => session::Axis::Vertical,
            },
            ratio: *ratio,
            a: Box::new(node_layout(a, panes)),
            b: Box::new(node_layout(b, panes)),
        },
        pane_grid::Node::Pane(pane) => {
            session::Layout::Pane(panes.get(*pane).and_then(Pane::thread).cloned())
        }
    }
}

fn restore(
    layout: &session::Layout,
    default: crate::config::messages::Layout,
) -> (pane_grid::Configuration<Pane>, Vec<Thread>) {
    match layout {
        session::Layout::Split { axis, ratio, a, b } => {
            let (a, mut threads) = restore(a, default);
            let (b, more) = restore(b, default);
            threads.extend(more);
            (
                pane_grid::Configuration::Split {
                    axis: match axis {
                        session::Axis::Horizontal => pane_grid::Axis::Horizontal,
                        session::Axis::Vertical => pane_grid::Axis::Vertical,
                    },
                    ratio: *ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                },
                threads,
            )
        }
        session::Layout::Pane(thread) => {
            let pane = match thread {
                Some(thread) => Pane::chat(thread.clone(), default),
                None => Pane::empty(default),
            };
            (
                pane_grid::Configuration::Pane(pane),
                thread.iter().cloned().collect(),
            )
        }
    }
}
