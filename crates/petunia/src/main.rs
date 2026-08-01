mod actions;
mod assets;
mod bridge;
mod demo;
mod favourites;
mod menus;
mod notify;
mod session;
mod store;
mod theme;
mod ui;

use std::sync::Arc;

use petunia_config as config;
use petunia_media::audio;

use gpui::prelude::*;
use gpui::Focusable;
use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size};
use gpui_component::Root;

use session::Session;
use store::Store;
use ui::workspace::Workspace;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,petunia=info".into()),
        )
        .init();

    // Carries the icon set; without it every icon renders as nothing.
    let app = gpui_platform::application().with_assets(assets::Assets);

    app.run(|cx: &mut App| {
        let loaded = config::load();
        install(&loaded, cx);
        // The three items a menu bar offers that belong to the application rather
        // than to a window, so they are answered here and nowhere else.
        cx.on_action(|_: &actions::Quit, cx| cx.quit());
        cx.on_action(|_: &actions::Hide, cx| cx.hide());
        cx.on_action(|_: &actions::HideOthers, cx| cx.hide_other_apps());
        menus::install(cx);
        notify::name_the_application();

        let config = Arc::new(loaded.config);
        let store = cx.new(|_| Store::new(config.clone()));
        let player = audio::Player::start();
        if demo::enabled() {
            demo::install(store.clone(), cx);
        } else {
            bridge::spawn(store.clone(), cx);
        }
        watch_config(store.clone(), cx);

        let blurred = config.sidebar.blurred();
        let session = Session::load();
        let bounds = Bounds::centered(
            None,
            size(px(session.window.width), px(session.window.height)),
            cx,
        );

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // The traffic lights float over petunia's own header strip,
                // which is where the panel toggles live. Only macOS draws
                // them; elsewhere the platform titlebar has no fixed control
                // cluster to clear.
                titlebar: Some(TitlebarOptions {
                    title: Some("Petunia".into()),
                    appears_transparent: true,
                    traffic_light_position: if cfg!(target_os = "macos") {
                        Some(point(px(14.0), px(14.0)))
                    } else {
                        None
                    },
                }),
                window_min_size: Some(size(px(560.0), px(420.0))),
                // Whole-window, because that is the only granularity there is:
                // a vibrancy layer goes behind everything, and what shows
                // through it is whatever is drawn translucent on top. Only the
                // conversation list is.
                // Transparent rather than Blurred: gpui's blur is a whole-window
                // view using the `Selection` material, which is a tint. The
                // conversation list puts its own behind itself, with the
                // material a sidebar is meant to have. See `ui::vibrancy`.
                window_background: match blurred {
                    true => gpui::WindowBackgroundAppearance::Transparent,
                    false => gpui::WindowBackgroundAppearance::Opaque,
                },
                ..Default::default()
            },
            |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(store, player, window, cx));
                // Nothing else claims focus on launch, and without it the
                // keymap has no path to dispatch along.
                window.focus(&workspace.read(cx).focus_handle(cx), cx);

                cx.new(|cx| {
                    let mut root = Root::new(workspace, window, cx);
                    // The widget library's root paints the theme background
                    // across the whole window, which would sit on top of the
                    // vibrancy layer and hide it completely. Nothing else needs
                    // it: every column paints its own.
                    if blurred {
                        root.style().background = Some(gpui::transparent_black().into());
                    }
                    root
                })
            },
        )
        .expect("open the petunia window");
    });
}

/// Applies a freshly read config. The order matters and is not obvious:
/// `actions::bind` clears **every** keybinding, including the ones the widget
/// library installs for its own text input, so the library has to be initialised
/// after it or the composer loses backspace, enter and every arrow key. The
/// theme goes last because `gpui_component::init` seeds its own palette.
fn install(loaded: &config::Loaded, cx: &mut App) {
    actions::bind(&loaded.config.keys, cx);
    gpui_component::init(cx);
    theme::install((*loaded.theme).clone(), cx);

    for error in &loaded.errors {
        tracing::warn!(%error, "config problem");
    }
}

/// Re-reads the config whenever it or a theme file changes, so an edit applies
/// without a restart.
fn watch_config(store: gpui::Entity<Store>, cx: &mut App) {
    let Some((watcher, mut changes)) = config::watch::changes() else {
        return;
    };

    cx.spawn(async move |cx| {
        // The watcher stops the moment it is dropped, so it lives here.
        let _watcher = watcher;

        loop {
            use futures::StreamExt;
            if changes.next().await.is_none() {
                break;
            }
            config::watch::settle(&mut changes, cx.background_executor()).await;

            let loaded = config::load();
            cx.update(|cx| {
                install(&loaded, cx);
                let config = Arc::new(loaded.config.clone());
                store.update(cx, |store, cx| store.config_changed(config, cx));
            });
            tracing::info!("reloaded config");
        }
    })
    .detach();
}
