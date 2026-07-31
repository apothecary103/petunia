use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{self, Receiver};
use notify::{RecursiveMode, Watcher};

/// Fires once per settled burst of writes to the config or a theme file. The
/// watcher is returned alongside because dropping it stops the stream.
///
/// Watches the *directory*, not the file: editors save by writing a temporary
/// file and renaming it, which removes the inode a file watch is holding.
pub fn changes() -> Option<(impl Watcher, Receiver<()>)> {
    let (sender, receiver) = mpsc::channel(16);

    // The callback is sync, so it hands events over the channel rather than
    // awaiting.
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && interesting(&event)
        {
            let _ = sender.clone().try_send(());
        }
    });

    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(%error, "could not start the config watcher; hot reload is off");
            return None;
        }
    };

    let dir = super::dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Err(error) = watcher.watch(&dir, RecursiveMode::Recursive) {
        tracing::warn!(%error, "could not watch the config directory");
        return None;
    }

    Some((watcher, receiver))
}

/// Waits for the writes to stop arriving before reporting a change, because one
/// save often lands as several events.
pub async fn settle(receiver: &mut Receiver<()>) {
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
    event
        .paths
        .iter()
        .any(|path| path.extension().is_some_and(|extension| extension == "toml"))
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
