use gpui::prelude::*;
use gpui::{App, Context, Entity, Focusable as _, SharedString, Subscription, Window, div, px};
use gpui_component::{ActiveTheme, IconName};

use super::conversation::{Conversation, Raise, Viewing};
use super::details::{self, Details};
use super::help::{self, Help};
use super::kit;
use super::linking::Linking;
use super::menu::{self, Menu};
use super::notice::Notices;
use super::palette::{Dismissed, Switcher};
use super::prompt::{self, Prompt};
use super::search::{self, Scope, Search};
use super::settings::{self, Settings};
use super::sidebar::Sidebar;
use super::viewer::{self, Viewer};
use crate::actions;
use crate::session::Session;
use crate::store::{Store, StoreEvent};
use crate::theme::ActivePalette;

/// Tall enough to clear the traffic lights, which macOS draws at a fixed size.
pub const TITLE_BAR: f32 = 40.0;

/// The root view. Shows the linking screen until an account exists, then the
/// conversation shell.
pub struct Workspace {
    store: Entity<Store>,
    player: crate::audio::Player,
    /// Actions dispatch along the focus path, so the root has to be in it or
    /// nothing bound to a key ever reaches this view.
    focus: gpui::FocusHandle,
    screen: Screen,
    session: Session,
    /// Present only while it is up, so nothing renders or listens otherwise.
    switcher: Option<Entity<Switcher>>,
    /// The full-size picture, over everything else. One at a time, so Escape
    /// never has to choose.
    viewer: Option<Entity<Viewer>>,
    help: Option<Entity<Help>>,
    search: Option<Entity<Search>>,
    menu: Option<Entity<Menu>>,
    settings: Option<Entity<Settings>>,
    prompt: Option<Entity<Prompt>>,
    /// Always present, and draws nothing until something has gone wrong.
    notices: Entity<Notices>,
    /// What the window is called, so the platform is only told when it changes.
    titled: String,
    _subscriptions: Vec<Subscription>,
}

enum Screen {
    Linking(Entity<Linking>),
    Main {
        sidebar: Entity<Sidebar>,
        conversation: Entity<Conversation>,
        details: Entity<Details>,
    },
}

impl Workspace {
    pub fn new(
        store: Entity<Store>,
        player: crate::audio::Player,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let linking = cx.new(|cx| Linking::new(store.clone(), cx));

        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe_in(&store, window, Self::on_store_event),
        ];

        let notices = cx.new(|_| Notices::default());
        let mut workspace = Self {
            store: store.clone(),
            player,
            focus: cx.focus_handle(),
            screen: Screen::Linking(linking),
            session: Session::load(),
            switcher: None,
            viewer: None,
            help: None,
            search: None,
            menu: None,
            settings: None,
            prompt: None,
            notices,
            titled: String::new(),
            _subscriptions: subscriptions,
        };

        // An already-linked store never emits `Linked` a second time, so the
        // shell has to come up if the account is there on the first read.
        if store.read(cx).state().is_some() {
            workspace.enter_main(window, cx);
        }
        workspace
    }

    fn enter_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sidebar = cx.new(|cx| Sidebar::new(self.store.clone(), cx));
        let conversation = cx.new(|cx| {
            Conversation::new(self.store.clone(), self.player.clone(), window, cx)
        });
        let details = cx.new(|cx| Details::new(self.store.clone(), cx));
        cx.subscribe_in(&details, window, |this, _, event: &details::Viewing, window, cx| {
            this.view_media(event.0.clone(), window, cx)
        })
        .detach();

        // A picture asked for full size opens over everything, so the viewer is
        // the workspace's rather than the conversation column's.
        cx.subscribe_in(&conversation, window, |this, _, event: &Viewing, window, cx| {
            this.view_media(event.0.clone(), window, cx)
        })
        .detach();
        cx.subscribe_in(&conversation, window, |this, _, raise: &Raise, window, cx| {
            this.raise_menu(raise.take(), raise.at, window, cx);
        })
        .detach();

        if let Some(thread) = self.session.active.clone() {
            self.store
                .update(cx, |store, cx| store.activate(thread, cx));
        }

        self.screen = Screen::Main {
            sidebar,
            conversation,
            details,
        };
    }

    fn on_store_event(
        &mut self,
        _store: &Entity<Store>,
        event: &StoreEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            StoreEvent::Linked => {
                self.enter_main(window, cx);
                cx.notify();
            }
            StoreEvent::Menu { thread, at } => {
                self.open_menu(thread.clone(), *at, window, cx);
            }
            // Clicking a name is a request to see it, so the panel opens
            // itself rather than making you find the toggle.
            StoreEvent::Inspecting => {
                if !self.session.details.open {
                    self.session.details.open = true;
                    self.session.save();
                }
                cx.notify();
            }
            // A failure nobody is told about is a failure that looks like
            // nothing happening.
            StoreEvent::Failed(message) => {
                let message = message.clone();
                self.notices
                    .update(cx, |notices, cx| notices.raise(message, cx));
            }
            _ => {}
        }
    }

    /// cmd+k. Raising it again while it is up refocuses and clears it, which is
    /// what pressing the shortcut twice is asking for.
    fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Main { .. }) {
            return;
        }

        let switcher = self.switcher.get_or_insert_with(|| {
            let switcher = cx.new(|cx| Switcher::new(self.store.clone(), cx));
            cx.subscribe(&switcher, |this, _, _: &Dismissed, cx| {
                this.switcher = None;
                cx.notify();
            })
            .detach();
            switcher
        });

        switcher.update(cx, |switcher, cx| switcher.reset(window, cx));
        cx.notify();
    }

    /// Shows a menu somebody else built. Whoever raised it knows what belongs on
    /// it; the workspace only knows where one can fit.
    fn raise_menu(
        &mut self,
        items: Vec<menu::Item>,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if items.is_empty() {
            return;
        }
        let raised = cx.new(|cx| Menu::new(items, at, cx));

        cx.subscribe(&raised, |this, _, _: &menu::Dismissed, cx| {
            this.menu = None;
            cx.notify();
        })
        .detach();
        window.focus(&raised.read(cx).focus_handle(cx), cx);

        self.menu = Some(raised);
        cx.notify();
    }

    /// cmd+, opens the preferences. It edits `config.toml`, so everything it
    /// changes arrives through the same reload a hand edit would.
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self
            .settings
            .get_or_insert_with(|| {
                let settings = cx.new(|cx| Settings::new(self.store.clone(), cx));
                cx.subscribe(&settings, |this, _, _: &settings::Dismissed, cx| {
                    this.settings = None;
                    cx.notify();
                })
                .detach();
                settings
            })
            .clone();

        window.focus(&settings.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// The menu for a conversation, built from what is true about it now.
    fn open_menu(
        &mut self,
        thread: crate::data::Thread,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let flags = store.read(cx).flags(&thread);
        let folders = store
            .read(cx)
            .state()
            .map(|state| state.index.folders())
            .unwrap_or_default();
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let apply: menu::thread::Apply = {
            let thread = thread.clone();
            std::rc::Rc::new(move |flags, _, cx| {
                store.update(cx, |store, cx| store.set_flags(thread.clone(), flags, cx));
            })
        };
        let create: menu::thread::Create = {
            let this = cx.entity();
            let thread = thread.clone();
            std::rc::Rc::new(move |window, cx| {
                let thread = thread.clone();
                this.update(cx, |this, cx| this.name_folder(thread, window, cx));
            })
        };

        let items = menu::thread::items(&flags, &folders, now, apply, create);
        self.raise_menu(items, at, window, cx);
    }

    /// Asks what to call a new folder, and puts the conversation in it.
    fn name_folder(
        &mut self,
        thread: crate::data::Thread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = cx.new(|cx| {
            Prompt::new("New folder", "Work, Family, …", "Create", window, cx)
        });

        cx.subscribe(&prompt, |this, _, _: &prompt::Dismissed, cx| {
            this.prompt = None;
            cx.notify();
        })
        .detach();
        cx.subscribe(&prompt, move |this, _, named: &prompt::Answered, cx| {
            let flags = crate::data::index::Flags {
                folder: Some(named.0.clone()),
                // A conversation you have just filed is one you want to see.
                archived: false,
                ..this.store.read(cx).flags(&thread)
            };
            this.store
                .update(cx, |store, cx| store.set_flags(thread.clone(), flags, cx));
        })
        .detach();

        prompt.update(cx, |prompt, cx| prompt.take_focus(window, cx));
        self.prompt = Some(prompt);
        cx.notify();
    }

    /// cmd+f searches everywhere; cmd+shift+f searches what is on screen. One
    /// surface either way, because they differ only in what they ask.
    fn open_search(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Main { .. }) {
            return;
        }

        let search = self.search.get_or_insert_with(|| {
            let store = self.store.clone();
            let search = cx.new(|cx| Search::new(store, scope.clone(), window, cx));

            cx.subscribe(&search, |this, _, _: &search::Dismissed, cx| {
                this.search = None;
                cx.notify();
            })
            .detach();
            cx.subscribe_in(
                &search,
                window,
                |this, _, chosen: &search::Chosen, _, cx| {
                    let thread = chosen.0.thread.clone();
                    this.store.update(cx, |store, cx| store.activate(thread, cx));
                },
            )
            .detach();
            search
        });

        search.update(cx, |search, cx| search.reset(scope, window, cx));
        window.focus(&search.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// What a scoped search is scoped to, or everywhere when nothing is open.
    fn thread_scope(&self, cx: &App) -> Scope {
        match self.store.read(cx).active() {
            Some(thread) => Scope::Thread(thread.clone()),
            None => Scope::Everywhere,
        }
    }

    /// Opens a picture full size, with everything else in the thread beside it.
    fn view_media(&mut self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let reel = self.thread_media(cx);
        let viewer = cx.new(|cx| Viewer::new(reel, &path, cx));

        cx.subscribe(&viewer, |this, _, _: &viewer::Dismissed, cx| {
            this.viewer = None;
            cx.notify();
        })
        .detach();
        window.focus(&viewer.read(cx).focus_handle(cx), cx);

        self.viewer = Some(viewer);
        cx.notify();
    }

    /// Every picture in the loaded page, oldest first, so the rail reads the way
    /// the conversation does.
    fn thread_media(&self, cx: &App) -> Vec<std::path::PathBuf> {
        use crate::data::attachment::{Blob, Kind};

        let store = self.store.read(cx);
        let Some(history) = store
            .active()
            .and_then(|thread| store.state()?.history(thread))
        else {
            return Vec::new();
        };

        history
            .messages()
            .iter()
            .flat_map(|message| message.attachments.iter())
            .filter(|attached| matches!(attached.kind, Kind::Image { .. } | Kind::Video { .. }))
            .filter_map(|attached| match &attached.blob {
                Blob::Cached(path) => Some(path.clone()),
                _ => None,
            })
            .collect()
    }

    /// cmd+/. Generated from the bindings actually in force, so a rebound key is
    /// never described wrongly.
    fn open_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bindings = self.store.read(cx).config.keys.listing();
        let help = cx.new(|cx| Help::new(bindings, cx));

        cx.subscribe(&help, |this, _, _: &help::Dismissed, cx| {
            this.help = None;
            cx.notify();
        })
        .detach();
        window.focus(&help.read(cx).focus_handle(cx), cx);

        self.help = Some(help);
        cx.notify();
    }

    fn step_conversation(&mut self, forward: bool, cx: &mut Context<Self>) {
        let next = self.store.read(cx).adjacent(forward);
        if let Some(thread) = next {
            self.store.update(cx, |store, cx| store.activate(thread, cx));
        }
    }

    fn conversation(&self) -> Option<&Entity<Conversation>> {
        match &self.screen {
            Screen::Main { conversation, .. } => Some(conversation),
            Screen::Linking(_) => None,
        }
    }

    /// Escape, in the order a person means it: the overlay first, then whatever
    /// the composer is carrying.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.menu.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.search.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.settings.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.help.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.viewer.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.switcher.take().is_some() {
            cx.notify();
            return;
        }
        let Some(conversation) = self.conversation().cloned() else {
            return;
        };
        conversation.update(cx, |conversation, cx| {
            conversation
                .composer()
                .clone()
                .update(cx, |composer, cx| composer.cancel(window, cx));
        });
        cx.notify();
    }

    fn with_conversation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        act: impl FnOnce(&mut Conversation, &mut Window, &mut Context<Conversation>),
    ) {
        let Some(conversation) = self.conversation().cloned() else {
            return;
        };
        conversation.update(cx, |conversation, cx| act(conversation, window, cx));
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.session.sidebar.open = !self.session.sidebar.open;
        self.session.save();
        cx.notify();
    }

    fn toggle_details(&mut self, cx: &mut Context<Self>) {
        self.session.details.open = !self.session.details.open;
        self.session.save();
        cx.notify();
    }

    /// The unread count belongs in the window title until there is somewhere
    /// better to put it.
    pub fn title(&self, cx: &App) -> String {
        let unread: u32 = self
            .store
            .read(cx)
            .state()
            .map(|state| state.index.total_unread())
            .unwrap_or(0);

        match unread {
            0 => "Petunia".into(),
            unread => format!("({unread}) Petunia"),
        }
    }
}

impl gpui::Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let border = cx.theme().border;

        let title = self.title(cx);
        if title != self.titled {
            window.set_window_title(&title);
            self.titled = title;
        }

        let body = match &self.screen {
            Screen::Linking(linking) => linking.clone().into_any_element(),
            Screen::Main {
                sidebar,
                conversation,
                details,
            } => {
                let title = self
                    .store
                    .read(cx)
                    .active()
                    .and_then(|thread| {
                        self.store
                            .read(cx)
                            .state()
                            .map(|state| state.title(thread))
                    })
                    .unwrap_or_default();

                // The sidebar runs the full height of the window and the
                // traffic lights float over its own top padding, so the header
                // belongs to the conversation column rather than spanning
                // everything. A full-width strip left a dead band above the
                // sidebar, which is what made the titlebar look wrong.
                div()
                    .size_full()
                    .flex()
                    .when(self.session.sidebar.open, |this| {
                        this.child(
                            div()
                                .w(px(self.session.sidebar.width))
                                .flex_none()
                                .border_r_1()
                                .border_color(border)
                                .child(sidebar.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(self.header(&title, &palette, cx))
                            .child(div().flex_1().min_h_0().child(conversation.clone())),
                    )
                    .when(self.session.details.open, |this| {
                        this.child(
                            div()
                                .w(px(self.session.details.width))
                                .flex_none()
                                .border_l_1()
                                .border_color(border)
                                .bg(palette.surface)
                                .child(details.clone()),
                        )
                    })
                    .into_any_element()
            }
        };

        div()
            .track_focus(&self.focus)
            .size_full()
            .bg(palette.background)
            .text_color(palette.text)
            .on_action(cx.listener(|this, _: &actions::ToggleSidebar, _, cx| {
                this.toggle_sidebar(cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ToggleDetails, _, cx| {
                this.toggle_details(cx)
            }))
            .on_action(cx.listener(|this, _: &actions::QuickSwitcher, window, cx| {
                this.open_switcher(window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::Help, window, cx| {
                this.open_help(window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::Settings, window, cx| {
                this.open_settings(window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::Search, window, cx| {
                this.open_search(Scope::Everywhere, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SearchThread, window, cx| {
                let scope = this.thread_scope(cx);
                this.open_search(scope, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::Cancel, window, cx| {
                this.cancel(window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::FocusComposer, window, cx| {
                this.with_conversation(window, cx, |conversation, window, cx| {
                    conversation
                        .composer()
                        .clone()
                        .update(cx, |composer, cx| composer.focus(window, cx));
                })
            }))
            .on_action(cx.listener(|this, _: &actions::ReplyToLast, window, cx| {
                this.with_conversation(window, cx, Conversation::reply_to_last)
            }))
            .on_action(cx.listener(|this, _: &actions::EditLast, window, cx| {
                this.with_conversation(window, cx, Conversation::edit_last)
            }))
            .on_action(cx.listener(|this, _: &actions::AttachFile, window, cx| {
                this.with_conversation(window, cx, |conversation, window, cx| {
                    conversation
                        .composer()
                        .clone()
                        .update(cx, |composer, cx| composer.pick_files(window, cx));
                })
            }))
            .on_action(cx.listener(|this, _: &actions::NextConversation, _, cx| {
                this.step_conversation(true, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::PreviousConversation, _, cx| {
                this.step_conversation(false, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::NextUnread, _, cx| {
                let next = this.store.read(cx).next_unread();
                if let Some(thread) = next {
                    this.store.update(cx, |store, cx| store.activate(thread, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &actions::MarkRead, _, cx| {
                let thread = this.store.read(cx).active().cloned();
                if let Some(thread) = thread {
                    this.store.update(cx, |store, _| store.mark_read(thread));
                }
            }))
            .child(body)
            .children(self.switcher.clone())
            .children(self.viewer.clone())
            .children(self.help.clone())
            .children(self.search.clone())
            .children(self.settings.clone())
            .children(self.menu.clone())
            .children(self.prompt.clone())
            .child(self.notices.clone())
    }
}

impl Workspace {
    /// A thin strip carrying the conversation's name and the panel toggles, in
    /// place of the reference's tab bar.
    fn header(
        &self,
        title: &str,
        palette: &crate::config::Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // No border underneath: the reference separates the columns, not the
        // strip, and a rule here cuts the window in half.
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            // Clears the traffic lights when the sidebar is not there to.
            .when(!self.session.sidebar.open, |this| this.pl(px(84.0)))
            .h(px(TITLE_BAR))
            .flex_none()
            .child(kit::icon_button(
                "toggle-sidebar",
                if self.session.sidebar.open {
                    IconName::PanelLeftClose
                } else {
                    IconName::PanelLeftOpen
                },
                palette,
                cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text)
                    .child(SharedString::from(title.to_owned())),
            )
            .child(kit::icon_button(
                "toggle-details",
                if self.session.details.open {
                    IconName::PanelRightClose
                } else {
                    IconName::PanelRightOpen
                },
                palette,
                cx.listener(|this, _, _, cx| this.toggle_details(cx)),
            ))
    }
}

