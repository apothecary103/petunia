//! Playback. Each half owns its output device on a thread the window does not
//! touch, and neither knows the framework exists.

pub mod audio;
pub mod song;
pub mod video;
