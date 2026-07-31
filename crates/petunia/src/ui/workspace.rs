use gpui::prelude::*;
use gpui::{App, Context, Entity, Focusable as _, SharedString, Subscription, Window, div, px};
use gpui_component::{ActiveTheme, IconName};

use super::conversation::{Conversation, Forwarding, Inspecting, Raise, Viewing};
use super::details::{self, Details};
use super::editor::{self, Editor};
use super::forward::{self, Forward};
use super::help::{self, Help};
use super::kit;
use super::linking::Linking;
use super::menu::{self, Menu};
use super::notice::Notices;
use super::palette::{Dismissed, Switcher};
use super::confirm::{self, Confirm};
use super::prompt::{self, Prompt};
use super::raw::{self, Raw};
use super::search::{self, Scope, Search};
use super::settings::{self, Settings};
use super::themes::{self, Themes};
use super::sidebar::Sidebar;
use super::viewer::{self, Viewer};
use crate::actions;
use crate::session::Session;
use crate::store::{Store, StoreEvent};
use crate::theme::ActivePalette;

/// Tall enough to clear the traffic lights, which macOS draws at a fixed size.
pub const TITLE_BAR: f32 = 40.0;

/// Wide enough to clear them too, for whatever is leftmost when the sidebar is
/// not there to hold them.
const TRAFFIC_LIGHTS: f32 = 84.0;

/// The narrowest the conversation column may be squeezed to before a side panel
/// gives way. Below this the avatar gutter and the reading column stop being a
/// column, and the window minimum is narrower than the two panels put together
/// -- so without this, dragging the window small leaves nothing between them.
const MIN_CONVERSATION: f32 = 420.0;

/// The collapsed list: avatars, and nothing that needs a line of text. Wide
/// enough to clear the traffic lights, which float over its own top padding at a
/// size macOS picks -- their right edge lands at about 66 -- and would otherwise
/// spill onto the conversation column.
pub const RAIL: f32 = 80.0;

/// What the divider may be dragged to. Below the snap point the list collapses
/// to the rail rather than becoming a column too narrow to read.
const MIN_SIDEBAR: f32 = 180.0;
const MAX_SIDEBAR: f32 = 480.0;
const SNAP_TO_RAIL: f32 = 150.0;

/// How wide a grab is. Drawn as a hairline, so this is the invisible margin
/// either side of it -- a one-pixel target is not one.
const HANDLE: f32 = 6.0;

/// The root view. Shows the linking screen until an account exists, then the
/// conversation shell.
pub struct Workspace {
    store: Entity<Store>,
    player: petunia_media::audio::Player,
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
    confirm: Option<Entity<Confirm>>,
    themes: Option<Entity<Themes>>,
    editor: Option<Entity<Editor>>,
    /// Where a message is being sent on to, and what the wire said about one.
    forward: Option<Entity<Forward>>,
    raw: Option<Entity<Raw>>,
    /// Always present, and draws nothing until something has gone wrong.
    notices: Entity<Notices>,
    /// The divider being dragged, while it is being dragged. The preference is
    /// only written on release: a drag reports every frame and `config.toml` is
    /// not a log.
    dragging: Option<Drag>,
    /// What the window is called, so the platform is only told when it changes.
    titled: String,
    _subscriptions: Vec<Subscription>,
}

/// A divider being dragged. `grab` is how far the pointer was from the edge when
/// it took hold, so the edge follows the pointer rather than jumping to it —
/// without it, the handle being a few pixels wide would make every click on it a
/// few pixels of resize.
#[derive(Debug, Clone, Copy)]
struct Drag {
    grab: f32,
    asked: f32,
}

/// Which side panels this frame draws. Not the same as what the session asks
/// for: a window too narrow to hold them lends their width to the conversation.
#[derive(Debug, Clone, Copy)]
struct Panels {
    sidebar: bool,
    details: bool,
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
        player: petunia_media::audio::Player,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let linking = cx.new(|cx| Linking::new(store.clone(), cx));

        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe_in(&store, window, Self::on_store_event),
            // Remembered as it happens, written once on the way out: a drag
            // reports every frame, and the session file is not a log.
            cx.observe_window_bounds(window, |this, window, _| this.remember_size(window)),
            cx.on_app_quit(|this, _| {
                this.session.save();
                async {}
            }),
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
            confirm: None,
            themes: None,
            editor: None,
            forward: None,
            raw: None,
            notices,
            dragging: None,
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
        let rail = self.session.sidebar.rail;
        let sidebar = cx.new(|cx| Sidebar::new(self.store.clone(), rail, cx));
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
        cx.subscribe_in(
            &conversation,
            window,
            |this, _, forwarding: &Forwarding, window, cx| {
                this.open_forward(forwarding.0, window, cx)
            },
        )
        .detach();
        cx.subscribe_in(
            &conversation,
            window,
            |this, _, inspecting: &Inspecting, window, cx| {
                this.open_raw(inspecting.0, window, cx)
            },
        )
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

    /// The whole file, for the keys that have no control of their own.
    fn open_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| Editor::new(window, cx));

        cx.subscribe(&editor, |this, _, _: &editor::Dismissed, cx| {
            this.editor = None;
            cx.notify();
        })
        .detach();
        editor.update(cx, |editor, cx| editor.take_focus(window, cx));

        self.editor = Some(editor);
        cx.notify();
    }

    /// A theme is a palette, not a name, so this previews as the selection
    /// moves and puts back what you arrived with if you leave without choosing.
    fn open_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picker = self.themes.get_or_insert_with(|| {
            let picker = cx.new(|cx| Themes::new(self.store.clone(), cx));
            cx.subscribe(&picker, |this, _, _: &themes::Dismissed, cx| {
                this.close_themes(cx);
            })
            .detach();
            picker
        });

        picker.update(cx, |picker, cx| picker.reset(window, cx));
        cx.notify();
    }

    /// Closing the picker puts back what was in force unless a theme was
    /// actually chosen. Both ways out come through here -- the picker's own
    /// Escape and the workspace's -- because one of them forgetting to restore
    /// would make looking at a theme the same as picking it.
    fn close_themes(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.themes.take() else {
            return;
        };
        if let Some(restore) = picker.read(cx).abandoned() {
            self.store.update(cx, |store, cx| {
                let mut config = (*store.config).clone();
                config.theme = restore.clone();
                store.config_changed(std::sync::Arc::new(config), cx);
            });
            crate::theme::install(petunia_config::theme::load(&restore).0, cx);
        }
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
                cx.subscribe_in(
                    &settings,
                    window,
                    |this, _, _: &settings::EditFile, window, cx| {
                        this.open_editor(window, cx)
                    },
                )
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
        thread: petunia_data::Thread,
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

        let delete: menu::thread::Delete = {
            let this = cx.entity();
            std::rc::Rc::new(move |window, cx| {
                let thread = thread.clone();
                this.update(cx, |this, cx| this.confirm_delete(thread, window, cx));
            })
        };

        let items = menu::thread::items(&flags, &folders, now, apply, create, delete);
        self.raise_menu(items, at, window, cx);
    }

    /// Asks before forgetting a conversation, and says what that means: the
    /// messages are this device's copy, and deleting them cannot reach the phone.
    fn confirm_delete(
        &mut self,
        thread: petunia_data::Thread,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .store
            .read(cx)
            .state()
            .map(|state| state.title(&thread))
            .unwrap_or_default();

        let confirm = cx.new(|cx| {
            Confirm::new(
                format!("Delete your conversation with {name}?"),
                "Every message in it is removed from this device, along with what \
                 petunia remembers about it. This cannot be undone, and it does not \
                 delete anything from your phone or for anyone else.",
                "Delete",
                cx,
            )
        });

        cx.subscribe(&confirm, |this, _, _: &confirm::Dismissed, cx| {
            this.confirm = None;
            cx.notify();
        })
        .detach();
        cx.subscribe(&confirm, move |this, _, _: &confirm::Confirmed, cx| {
            this.store
                .update(cx, |store, cx| store.delete_thread(thread.clone(), cx));
        })
        .detach();

        confirm.update(cx, |confirm, cx| confirm.take_focus(window, cx));
        self.confirm = Some(confirm);
        cx.notify();
    }

    /// Asks what to call a new folder, and puts the conversation in it.
    fn name_folder(
        &mut self,
        thread: petunia_data::Thread,
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
            let flags = petunia_data::index::Flags {
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

    /// Asks where a message is to be sent on to, and sends it there.
    fn open_forward(
        &mut self,
        target: petunia_data::MessageId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let summary = self
            .store
            .read(cx)
            .find(&target)
            .map(petunia_data::Message::summary)
            .unwrap_or_default();

        let picker = cx.new(|cx| Forward::new(self.store.clone(), target, summary, cx));

        cx.subscribe(&picker, |this, _, _: &forward::Dismissed, cx| {
            this.forward = None;
            cx.notify();
        })
        .detach();
        cx.subscribe(&picker, |this, _, picked: &forward::Picked, cx| {
            let (target, thread) = (picked.target, picked.thread.clone());
            this.store
                .update(cx, |store, cx| store.forward(target, thread.clone(), cx));
            // Taken to where it went, because a forward with no visible result
            // reads as one that did not happen.
            this.store.update(cx, |store, cx| store.activate(thread, cx));
        })
        .detach();

        picker.update(cx, |picker, cx| picker.take_focus(window, cx));
        self.forward = Some(picker);
        cx.notify();
    }

    /// What the wire said about one message, for the questions the drawing of it
    /// cannot answer.
    fn open_raw(
        &mut self,
        target: petunia_data::MessageId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.read(cx);
        let Some(message) = store.find(&target).cloned() else {
            return;
        };
        let sender = store
            .state()
            .map(|state| state.sender_name(message.sender()))
            .unwrap_or_default();

        let sheet = cx.new(|cx| Raw::new(&message, &sender, cx));

        cx.subscribe(&sheet, |this, _, _: &raw::Dismissed, cx| {
            this.raw = None;
            cx.notify();
        })
        .detach();

        sheet.update(cx, |sheet, cx| sheet.take_focus(window, cx));
        self.raw = Some(sheet);
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
                |this, _, chosen: &search::Chosen, window, cx| {
                    let hit = chosen.0.clone();
                    let target = petunia_data::MessageId {
                        timestamp: hit.timestamp,
                        sender: hit.sender,
                    };
                    this.store
                        .update(cx, |store, cx| store.activate(hit.thread, cx));
                    // Opening the conversation is only half of it: the answer is
                    // some way back up the thread, and finding it again by hand
                    // is what the search was for.
                    this.with_conversation(window, cx, move |conversation, _, cx| {
                        conversation.reveal(target, cx)
                    });
                },
            )
            .detach();
            search
        });

        // The query field takes the focus, not the sheet around it. Focusing the
        // sheet leaves the field looking active and swallowing nothing, which is
        // what made cmd+f open a search box that could not be typed into.
        search.update(cx, |search, cx| search.reset(scope, window, cx));
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
        use petunia_data::attachment::{Blob, Kind};

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
        if self.editor.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.raw.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.forward.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.themes.is_some() {
            self.close_themes(cx);
            window.focus(&self.focus, cx);
            return;
        }
        // Before the prompt and the menu: it is raised over both, and Escape
        // means the thing most recently put in front of you.
        if self.confirm.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
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

    /// Keeps the size the window would be restored to, which is what it should
    /// open at next time. Every `WindowBounds` variant carries that size, so a
    /// window quit while maximised comes back the size it was before.
    fn remember_size(&mut self, window: &Window) {
        let bounds = match window.window_bounds() {
            gpui::WindowBounds::Windowed(bounds)
            | gpui::WindowBounds::Maximized(bounds)
            | gpui::WindowBounds::Fullscreen(bounds) => bounds,
        };
        self.session.window = crate::session::WindowSize {
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
        };
    }

    /// Which panels are actually drawn. The session records what you asked for;
    /// this is what fits, and the toggles keep meaning what you asked for so a
    /// window dragged wide again brings the panels back with it.
    ///
    /// The details panel gives way first, and only then the sidebar: the list is
    /// how you get anywhere, and details without a list to leave by is worse
    /// than no details.
    fn panels(&self, window: &Window, cx: &App) -> Panels {
        let width = f32::from(window.viewport_size().width);
        let mut shown = Panels {
            sidebar: self.session.sidebar.open,
            details: self.session.details.open,
        };
        let sidebar = match shown.sidebar {
            true => self.sidebar_width(cx).1,
            false => 0.0,
        };

        if shown.details && width - sidebar - self.session.details.width < MIN_CONVERSATION {
            shown.details = false;
        }
        if shown.sidebar && width - sidebar < MIN_CONVERSATION {
            shown.sidebar = false;
        }
        shown
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.session.sidebar.open = !self.session.sidebar.open;
        self.session.save();
        cx.notify();
    }

    /// Whether the list is a rail this frame, and how wide it is drawn. While a
    /// drag is live that is whatever the pointer is asking for; otherwise it is
    /// the preference, or the rail when it is collapsed.
    fn sidebar_width(&self, cx: &App) -> (bool, f32) {
        match self.dragging {
            Some(drag) => resolve(drag.asked),
            None => (self.session.sidebar.rail, self.settled_width(cx)),
        }
    }

    /// The width the list rests at, which is the rail when it is collapsed and
    /// the preference otherwise.
    fn settled_width(&self, cx: &App) -> f32 {
        match self.session.sidebar.rail {
            true => RAIL,
            false => self
                .store
                .read(cx)
                .config
                .sidebar
                .width
                .clamp(MIN_SIDEBAR, MAX_SIDEBAR),
        }
    }

    fn grab_sidebar(&mut self, at: f32, cx: &mut Context<Self>) {
        let width = self.settled_width(cx);
        self.dragging = Some(Drag {
            grab: width - at,
            asked: width,
        });
        cx.notify();
    }

    /// Follows the pointer. Nothing is written yet, so a drag abandoned by
    /// letting go outside the window leaves the preference alone.
    fn drag_sidebar(&mut self, at: f32, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.dragging else {
            return;
        };
        drag.asked = at + drag.grab;
        cx.notify();
    }

    /// Keeps where the drag ended. A rail is session state and a width is a
    /// preference, so the two land in different files -- which is also why the
    /// rail is not simply a width of its own.
    fn drop_sidebar(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.dragging.take() else {
            return;
        };
        let (rail, width) = resolve(drag.asked);

        if self.session.sidebar.rail != rail {
            self.session.sidebar.rail = rail;
            self.session.save();
            self.set_rail(rail, cx);
        }
        if !rail {
            self.store.update(cx, |store, cx| {
                let mut config = (*store.config).clone();
                config.sidebar.width = width;
                if let Err(error) = petunia_config::write::save(&config) {
                    tracing::warn!(%error, "could not save the sidebar width");
                }
                store.config_changed(std::sync::Arc::new(config), cx);
            });
        }
        cx.notify();
    }

    fn set_rail(&self, rail: bool, cx: &mut Context<Self>) {
        if let Screen::Main { sidebar, .. } = &self.screen {
            sidebar.update(cx, |sidebar, cx| sidebar.collapse(rail, cx));
        }
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
            .map(|state| {
                state
                    .index
                    .total_unread(chrono::Utc::now().timestamp_millis() as u64)
            })
            .unwrap_or(0);

        match unread {
            0 => "Petunia".into(),
            unread => format!("({unread}) Petunia"),
        }
    }
}

/// A width the pointer asked for, as what is actually drawn.
fn resolve(asked: f32) -> (bool, f32) {
    match asked < SNAP_TO_RAIL {
        true => (true, RAIL),
        false => (false, asked.clamp(MIN_SIDEBAR, MAX_SIDEBAR)),
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
        let panels = self.panels(window, cx);
        let (_, sidebar_width) = self.sidebar_width(cx);

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
                    .when(panels.sidebar, |this| {
                        this.child(
                            div()
                                .relative()
                                .w(px(sidebar_width))
                                .flex_none()
                                .border_r_1()
                                .border_color(border)
                                .child(sidebar.clone())
                                .child(self.handle(&palette, cx)),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .bg(palette.background)
                            .child(self.header(&title, panels, &palette, cx))
                            .child(div().flex_1().min_h_0().child(conversation.clone())),
                    )
                    .when(panels.details, |this| {
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

        let translucent = self.store.read(cx).config.sidebar.blurred();
        if translucent {
            let showing = panels.sidebar && matches!(self.screen, Screen::Main { .. });
            super::vibrancy::sidebar(sidebar_width, showing, palette.is_light());
        }

        div()
            .track_focus(&self.focus)
            .size_full()
            .when(self.dragging.is_some(), |this| this.child(self.follow(cx)))
            // Left unpainted when the list is translucent: a background here
            // would cover the vibrancy layer the whole effect depends on.
            .when(!translucent, |this| this.bg(palette.background))
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
            .on_action(cx.listener(|this, _: &actions::ThemePicker, window, cx| {
                this.open_themes(window, cx)
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
            // Over the menu it was raised from, since that is what it answers.
            .children(self.confirm.clone())
            .children(self.themes.clone())
            .children(self.editor.clone())
            .children(self.forward.clone())
            .children(self.raw.clone())
            .child(self.notices.clone())
    }
}

impl Workspace {
    /// What the columns are separated by, once it can be dragged. The rule is
    /// the sidebar's own right border; this is the target just inside it, laid
    /// over the list rather than between the columns -- a strip of its own would
    /// move everything sideways the moment it appeared.
    fn handle(&self, palette: &petunia_config::Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let held = self.dragging.is_some();

        div()
            .id("resize-sidebar")
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(HANDLE))
            .flex()
            .justify_end()
            .cursor_col_resize()
            // Lit while it is being dragged, so the pointer is not the only thing
            // saying the gesture has started.
            .child(
                div()
                    .w_px()
                    .h_full()
                    .when(held, |this| this.bg(palette.border_focus)),
            )
            .when(!held, |this| {
                this.hover(|this| this.bg(kit::tinted(palette.border_focus)))
            })
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    // The handle lies over the list's right edge, so without this
                    // grabbing it would also open whichever conversation is
                    // behind it.
                    cx.stop_propagation();
                    this.grab_sidebar(f32::from(event.position.x), cx)
                }),
            )
    }

    /// Follows the pointer for the length of a drag.
    ///
    /// At the window level rather than through an element's own listeners: a div
    /// only hears a mouse move while it is the thing under the pointer, and a
    /// drag is precisely the gesture that leaves the handle behind — which is why
    /// the column moved only on release when this was an `on_mouse_move` on the
    /// root. A `canvas` is the way to reach `Window::on_mouse_event` without
    /// writing a whole element; it draws nothing and takes no space.
    fn follow(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let moved = cx.listener(|this: &mut Self, event: &gpui::MouseMoveEvent, _, cx| {
            this.drag_sidebar(f32::from(event.position.x), cx)
        });
        let released = cx.listener(|this: &mut Self, _: &gpui::MouseUpEvent, _, cx| {
            this.drop_sidebar(cx)
        });

        gpui::canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                    if phase.bubble() {
                        moved(event, window, cx);
                    }
                });
                window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, window, cx| {
                    if phase.bubble() {
                        released(event, window, cx);
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    /// A thin strip carrying the conversation's name and the panel toggles, in
    /// place of the reference's tab bar.
    fn header(
        &self,
        title: &str,
        panels: Panels,
        palette: &petunia_config::Theme,
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
            .when(!panels.sidebar, |this| this.pl(px(TRAFFIC_LIGHTS)))
            .h(px(TITLE_BAR))
            .flex_none()
            .child(kit::icon_button(
                "toggle-sidebar",
                if panels.sidebar {
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
                if panels.details {
                    IconName::PanelRightClose
                } else {
                    IconName::PanelRightOpen
                },
                palette,
                cx.listener(|this, _, _, cx| this.toggle_details(cx)),
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dragging the divider in towards the window edge collapses the list rather
    /// than leaving a column too narrow to read a name in.
    #[test]
    fn a_narrow_drag_snaps_to_the_rail() {
        assert_eq!(resolve(40.0), (true, RAIL));
        assert_eq!(resolve(SNAP_TO_RAIL - 1.0), (true, RAIL));
    }

    #[test]
    fn a_wide_drag_is_a_width_within_the_bounds() {
        assert_eq!(resolve(SNAP_TO_RAIL), (false, MIN_SIDEBAR));
        assert_eq!(resolve(300.0), (false, 300.0));
        assert_eq!(resolve(2000.0), (false, MAX_SIDEBAR));
    }
}

