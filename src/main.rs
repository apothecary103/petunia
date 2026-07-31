mod config;
mod data;
mod session;
mod signal;
mod ui;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,petunia=info".into()),
        )
        .init();

    let loaded = config::load();
    for error in &loaded.errors {
        tracing::warn!(%error, "config problem");
    }
    tracing::info!(theme = %loaded.theme.name, "config loaded");
}
