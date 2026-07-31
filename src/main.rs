mod actions;
mod config;
mod data;
mod session;
mod signal;
mod store;
mod theme;
mod ui;

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{App, Bounds, WindowBounds, WindowOptions, px, size};
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

    gpui_platform::application().run(|cx: &mut App| {
        gpui_component::init(cx);

        let loaded = config::load();
        theme::install((*loaded.theme).clone(), cx);
        actions::bind(&loaded.config.keys, cx);
        for error in &loaded.errors {
            tracing::warn!(%error, "config problem");
        }

        let config = Arc::new(loaded.config);
        let store = cx.new(|_| Store::new(config));
        signal::bridge::spawn(store.clone(), cx);
        watch_config(store.clone(), cx);

        let session = Session::load();
        let bounds = Bounds::centered(
            None,
            size(px(session.window.width), px(session.window.height)),
            cx,
        );

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(store, window, cx));
                cx.new(|cx| Root::new(workspace, window, cx))
            },
        )
        .expect("open the petunia window");
    });
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
            config::watch::settle(&mut changes).await;

            let loaded = config::load();
            cx.update(|cx| {
                theme::install((*loaded.theme).clone(), cx);
                actions::bind(&loaded.config.keys, cx);
                for error in &loaded.errors {
                    tracing::warn!(%error, "config problem");
                }
                let config = Arc::new(loaded.config);
                store.update(cx, |store, cx| store.config_changed(config, cx));
            });
            tracing::info!("reloaded config");
        }
    })
    .detach();
}
