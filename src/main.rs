mod app;
mod config;
mod data;
mod pane;
mod screen;
mod session;
mod signal;
mod theme;
mod widget;

use app::Petunia;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,petunia=info".into()),
        )
        .init();

    iced::daemon(Petunia::new, Petunia::update, Petunia::view)
        .title(Petunia::title)
        .subscription(Petunia::subscription)
        .theme(Petunia::theme)
        .default_font(iced::Font::MONOSPACE)
        .run()
}
