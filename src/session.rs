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
    pub layout: Option<Layout>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

/// Mirrors iced's pane grid so a workspace survives a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Layout {
    Split {
        axis: Axis,
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
    Pane(Option<Thread>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            window: WindowSize {
                width: 1024.0,
                height: 720.0,
            },
            layout: None,
        }
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
    fn reads_a_session_written_before_the_move() {
        let stored = r#"{
            "window": { "width": 1512.0, "height": 949.0 },
            "layout": { "Pane": { "Contact": { "Aci": "9946e398-4709-477e-baf2-4f6ab82bbad4" } } }
        }"#;

        let session: Session = serde_json::from_str(stored).unwrap();

        assert_eq!(session.window.width, 1512.0);
        let Some(Layout::Pane(Some(Thread::Contact(contact)))) = session.layout else {
            panic!("expected a single restored contact pane");
        };
        assert_eq!(
            contact.uuid().to_string(),
            "9946e398-4709-477e-baf2-4f6ab82bbad4"
        );
    }

    #[test]
    fn falls_back_to_defaults_for_a_missing_or_partial_file() {
        let session: Session = serde_json::from_str("{}").unwrap();

        assert_eq!(session.window.width, 1024.0);
        assert!(session.layout.is_none());
    }

    #[test]
    fn round_trips_a_split_layout() {
        let session = Session {
            layout: Some(Layout::Split {
                axis: Axis::Vertical,
                ratio: 0.4,
                a: Box::new(Layout::Pane(None)),
                b: Box::new(Layout::Pane(Some(Thread::Group([3u8; 32])))),
            }),
            ..Session::default()
        };

        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        let Some(Layout::Split { ratio, b, .. }) = restored.layout else {
            panic!("expected a split");
        };
        assert_eq!(ratio, 0.4);
        assert!(matches!(*b, Layout::Pane(Some(Thread::Group(_)))));
    }
}
