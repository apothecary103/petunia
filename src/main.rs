mod actions;
mod audio;
mod config;
mod data;
mod session;
mod signal;
mod store;
mod theme;
mod ui;
mod video;

use std::sync::Arc;

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
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);

    app.run(|cx: &mut App| {
        let loaded = config::load();
        install(&loaded, cx);

        let config = Arc::new(loaded.config);
        let store = cx.new(|_| Store::new(config.clone()));
        let player = audio::Player::start();
        signal::bridge::spawn(store.clone(), cx);
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
                // which is where the panel toggles live.
                titlebar: Some(TitlebarOptions {
                    title: Some("Petunia".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(14.0), px(14.0))),
                }),
                window_min_size: Some(size(px(560.0), px(420.0))),
                // Whole-window, because that is the only granularity there is:
                // a vibrancy layer goes behind everything, and what shows
                // through it is whatever is drawn translucent on top. Only the
                // conversation list is.
                window_background: match blurred {
                    true => gpui::WindowBackgroundAppearance::Blurred,
                    false => gpui::WindowBackgroundAppearance::Opaque,
                },
                ..Default::default()
            },
            |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(store, player, window, cx));
                // Nothing else claims focus on launch, and without it the
                // keymap has no path to dispatch along.
                window.focus(&workspace.read(cx).focus_handle(cx), cx);
                cx.new(|cx| Root::new(workspace, window, cx))
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
