//! Playback. Each half owns its output device on a thread the window does not
//! touch, and neither knows the framework exists.

pub mod audio;
pub mod recorder;
pub mod song;
pub mod waveform;
pub mod video;
