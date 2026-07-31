use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::data::Thread;

/// State petunia writes for itself. User preferences are a separate,
/// hand-edited file and never written from here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub window: WindowSize,
    /// The conversation to reopen on launch.
    pub active: Option<Thread>,
    #[serde(default = "sidebar")]
    pub sidebar: PanelState,
    #[serde(default = "details")]
    pub details: PanelState,
}

fn sidebar() -> PanelState {
    PanelState {
        open: true,
        width: 260.0,
    }
}

/// Closed until asked for: an empty panel taking a fifth of the window is worse
/// than no panel.
fn details() -> PanelState {
    PanelState {
        open: false,
        width: 300.0,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelState {
    pub open: bool,
    pub width: f32,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            window: WindowSize {
                width: 1024.0,
                height: 720.0,
            },
            active: None,
            sidebar: sidebar(),
            details: details(),
        }
    }
}

impl Default for PanelState {
    fn default() -> Self {
        sidebar()
    }
}

impl Session {
    pub fn load() -> Self {
        fs::read_to_string(path())
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let contents = serde_json::to_string_pretty(self).expect("session is serializable");
        if let Err(error) = fs::write(&path, contents) {
            warn!(%error, path = %path.display(), "failed to save session");
        }
    }
}

fn path() -> PathBuf {
    crate::config::dir().join("session.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_defaults_for_a_missing_or_partial_file() {
        let session: Session = serde_json::from_str("{}").unwrap();

        assert_eq!(session.window.width, 1024.0);
        assert!(session.active.is_none());
        assert!(session.sidebar.open);
        assert!(!session.details.open);
    }

    /// Sessions written by the iced build carry a `layout` pane tree that no
    /// longer exists; the window size in them is still worth keeping.
    #[test]
    fn a_session_from_the_pane_grid_still_loads() {
        let stored = r#"{
            "window": { "width": 1512.0, "height": 949.0 },
            "layout": { "Pane": { "Contact": { "Aci": "9946e398-4709-477e-baf2-4f6ab82bbad4" } } }
        }"#;

        let session: Session = serde_json::from_str(stored).unwrap();

        assert_eq!(session.window.width, 1512.0);
        assert_eq!(session.window.height, 949.0);
    }

    #[test]
    fn round_trips_the_active_thread_and_panels() {
        let session = Session {
            active: Some(Thread::Group([3u8; 32])),
            details: PanelState {
                open: true,
                width: 320.0,
            },
            ..Session::default()
        };

        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert!(matches!(restored.active, Some(Thread::Group(_))));
        assert!(restored.details.open);
        assert_eq!(restored.details.width, 320.0);
    }
}
