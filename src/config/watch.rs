use std::time::Duration;

use iced::Subscription;
use iced::futures::{SinkExt, Stream, StreamExt};
use notify::{RecursiveMode, Watcher};

/// Fires when the config or a theme file changes on disk.
pub fn changes() -> Subscription<()> {
    Subscription::run(stream)
}

/// Watches the *directory*, not the file: editors save by writing a temporary
/// file and renaming it, which removes the inode a file watch is holding.
fn stream() -> impl Stream<Item = ()> {
    iced::stream::channel(1, async |mut output| {
        let (sender, mut receiver) = iced::futures::channel::mpsc::channel(16);

        // The watcher must outlive the loop, and its callback is sync, so it
        // hands events over the channel rather than awaiting.
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event
                && interesting(&event)
            {
                let _ = sender.clone().try_send(());
            }
        });

        let Ok(mut watcher) = watcher else {
            tracing::warn!("could not start the config watcher; hot reload is off");
            std::future::pending::<()>().await;
            unreachable!("pending never resolves")
        };

        let dir = super::dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Err(error) = watcher.watch(&dir, RecursiveMode::Recursive) {
            tracing::warn!(%error, "could not watch the config directory");
        }

        loop {
            if receiver.next().await.is_none() {
                break;
            }
            // One save often lands as several events; collapse the burst.
            settle(&mut receiver).await;
            let _ = output.send(()).await;
        }

        drop(watcher);
    })
}

/// Waits for the writes to stop arriving before reporting a change.
async fn settle(receiver: &mut iced::futures::channel::mpsc::Receiver<()>) {
    const DEBOUNCE: Duration = Duration::from_millis(250);

    loop {
        match tokio::time::timeout(DEBOUNCE, receiver.next()).await {
            Ok(Some(())) => continue,
            Ok(None) | Err(_) => return,
        }
    }
}

fn interesting(event: &notify::Event) -> bool {
    use notify::EventKind;

    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }
    // session.json is ours; reacting to our own writes would loop.
    event.paths.iter().any(|path| {
        path.extension().is_some_and(|extension| extension == "toml")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{EventKind, ModifyKind};
    use std::path::PathBuf;

    fn modified(path: &str) -> notify::Event {
        notify::Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![PathBuf::from(path)],
            attrs: Default::default(),
        }
    }

    #[test]
    fn a_toml_write_is_interesting() {
        assert!(interesting(&modified("/cfg/petunia/config.toml")));
        assert!(interesting(&modified("/cfg/petunia/themes/nord.toml")));
    }

    /// petunia writes session.json itself, so reacting to it would reload on
    /// every window resize.
    #[test]
    fn our_own_session_file_is_ignored() {
        assert!(!interesting(&modified("/cfg/petunia/session.json")));
    }

    #[test]
    fn an_access_event_is_ignored() {
        let event = notify::Event {
            kind: EventKind::Access(notify::event::AccessKind::Read),
            paths: vec![PathBuf::from("/cfg/petunia/config.toml")],
            attrs: Default::default(),
        };

        assert!(!interesting(&event));
    }
}
