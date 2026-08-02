//! Playing voice notes and audio files.
//!
//! The output device is owned by a thread of its own rather than by a view: a
//! `cpal` stream is not `Send`, and it has to outlive any one message on screen.
//! Views ask through a channel and read a shared snapshot, which is the same
//! shape the Signal worker uses.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, RecvTimeoutError, channel};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use rodio::{Decoder, Source};
use tracing::warn;

/// How often the thread republishes where playback has got to. Fine enough that
/// a progress bar moves smoothly, coarse enough to cost nothing.
const TICK: Duration = Duration::from_millis(100);

enum Request {
    /// Starts this file, or pauses it if it is the one already playing.
    Toggle(PathBuf),
    /// Jumps to a fraction of the way through.
    Seek(f32),
    Stop,
}

/// What is playing and how far in. Cloned out per frame, so the views never
/// hold the lock.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playback {
    pub file: Option<PathBuf>,
    pub playing: bool,
    pub position: Duration,
    /// Absent for a format whose decoder cannot say, which is common enough for
    /// Signal's voice notes that the bar has to cope.
    pub duration: Option<Duration>,
}

impl Playback {
    /// How far through, as a fraction, or `None` when the length is unknown.
    pub fn fraction(&self) -> Option<f32> {
        let duration = self.duration?.as_secs_f32();
        (duration > 0.0).then(|| (self.position.as_secs_f32() / duration).clamp(0.0, 1.0))
    }

    pub fn is(&self, file: &std::path::Path) -> bool {
        self.file.as_deref() == Some(file)
    }
}

#[derive(Clone)]
pub struct Player {
    requests: Sender<Request>,
    playback: Arc<RwLock<Playback>>,
}

impl Player {
    pub fn start() -> Self {
        let (requests, incoming) = channel();
        let playback = Arc::new(RwLock::new(Playback::default()));

        std::thread::Builder::new()
            .name("petunia-audio".into())
            .spawn({
                let playback = playback.clone();
                move || serve(incoming, playback)
            })
            .expect("spawn the audio thread");

        Self { requests, playback }
    }

    pub fn playback(&self) -> Playback {
        self.playback.read().map(|state| state.clone()).unwrap_or_default()
    }

    pub fn toggle(&self, file: PathBuf) {
        let _ = self.requests.send(Request::Toggle(file));
    }

    pub fn seek(&self, fraction: f32) {
        let _ = self.requests.send(Request::Seek(fraction));
    }

    pub fn stop(&self) {
        let _ = self.requests.send(Request::Stop);
    }
}

fn serve(requests: Receiver<Request>, published: Arc<RwLock<Playback>>) {
    let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(error) => {
            warn!(%error, "no audio output; playback is unavailable");
            // Draining rather than returning, so `toggle` does not fail
            // silently into a closed channel and look like a lost click.
            while requests.recv().is_ok() {}
            return;
        }
    };
    let player = rodio::Player::connect_new(sink.mixer());
    let mut state = Playback::default();

    loop {
        match requests.recv_timeout(TICK) {
            Ok(Request::Toggle(file)) => toggle(&player, &mut state, file),
            Ok(Request::Seek(fraction)) => seek(&player, &state, fraction),
            Ok(Request::Stop) => {
                player.clear();
                state = Playback::default();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        if state.file.is_some() {
            state.position = player.get_pos();
            // A finished track is not a paused one: it goes back to the start
            // so pressing play again replays it rather than doing nothing.
            if player.empty() {
                state.playing = false;
                state.position = Duration::ZERO;
            } else {
                state.playing = !player.is_paused();
            }
        }
        if let Ok(mut published) = published.write() {
            *published = state.clone();
        }
    }
}

fn toggle(player: &rodio::Player, state: &mut Playback, file: PathBuf) {
    if state.is(&file) && !player.empty() {
        if player.is_paused() {
            player.play();
        } else {
            player.pause();
        }
        return;
    }

    player.clear();
    let source = match std::fs::File::open(&file).map_err(Error::Io).and_then(|file| {
        Decoder::new(std::io::BufReader::new(file)).map_err(Error::Decode)
    }) {
        Ok(source) => source,
        Err(error) => {
            warn!(%error, file = %file.display(), "cannot play this file");
            *state = Playback::default();
            return;
        }
    };

    // The decoder only knows the length of a format that stores one, which mp3,
    // flac and ogg do not: without this the bar never filled and a click on it
    // seeked nowhere, since `seek` has nothing to take a fraction of. The tags
    // know, having read the stream's own properties.
    let duration = source.total_duration().or_else(|| crate::song::read(&file)?.duration);

    *state = Playback {
        duration,
        file: Some(file),
        playing: true,
        position: Duration::ZERO,
    };
    player.append(source);
    player.play();
}

fn seek(player: &rodio::Player, state: &Playback, fraction: f32) {
    let Some(duration) = state.duration else {
        return;
    };
    let target = duration.mul_f32(fraction.clamp(0.0, 1.0));
    if let Err(error) = player.try_seek(target) {
        warn!(%error, "cannot seek in this file");
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Decode(#[from] rodio::decoder::DecoderError),
}

/// Signal ships a precomputed waveform with a voice note: one byte per bar, so
/// there is nothing to decode before the bars can be drawn. What arrives without
/// one is read from the file instead (`crate::waveform`), and only a sound that
/// could not be read at all falls back to the flat bar — which is honest about
/// knowing nothing rather than inventing a shape.
///
/// The loudest value in the range each bar covers rather than the one that
/// happens to land on its edge: a shape kept at a finer resolution than it is
/// drawn at, point-sampled, loses whichever peaks fall between the bars — which
/// on speech is most of them, and the strip flattens as the window narrows.
pub fn bars(waveform: Option<&[u8]>, wanted: usize) -> Vec<f32> {
    let Some(waveform) = waveform.filter(|waveform| !waveform.is_empty()) else {
        return vec![0.35; wanted];
    };

    (0..wanted)
        .map(|index| {
            let (from, to) = crate::waveform::span(index, wanted, waveform.len());
            let peak = waveform[from..to].iter().copied().max().unwrap_or(0);
            // A bar of nothing is invisible, and a run of them reads as a
            // rendering failure rather than as silence.
            (peak as f32 / 255.0).max(0.08)
        })
        .collect()
}

/// Seconds as a clock, which is the only way anyone reads a track length.
pub fn clock(duration: Duration) -> String {
    let total = duration.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fraction_needs_a_known_length() {
        let unknown = Playback {
            position: Duration::from_secs(5),
            ..Default::default()
        };
        assert_eq!(unknown.fraction(), None);

        let known = Playback {
            position: Duration::from_secs(5),
            duration: Some(Duration::from_secs(20)),
            ..Default::default()
        };
        assert_eq!(known.fraction(), Some(0.25));
    }

    /// A position past the end can happen while the decoder catches up, and a
    /// bar wider than its track looks broken.
    #[test]
    fn a_fraction_never_leaves_the_bar() {
        let over = Playback {
            position: Duration::from_secs(90),
            duration: Some(Duration::from_secs(20)),
            ..Default::default()
        };

        assert_eq!(over.fraction(), Some(1.0));
    }

    #[test]
    fn a_zero_length_track_reports_no_progress() {
        let empty = Playback {
            duration: Some(Duration::ZERO),
            ..Default::default()
        };

        assert_eq!(empty.fraction(), None);
    }

    #[test]
    fn bars_are_resampled_to_the_width_asked_for() {
        let waveform: Vec<u8> = (0..64).collect();

        assert_eq!(bars(Some(&waveform), 16).len(), 16);
        assert_eq!(bars(Some(&waveform), 100).len(), 100);
    }

    /// Nothing to draw still has to draw something, or an audio file with no
    /// waveform renders as an empty box.
    #[test]
    fn a_missing_waveform_still_produces_bars() {
        assert_eq!(bars(None, 8).len(), 8);
        assert_eq!(bars(Some(&[]), 8).len(), 8);
        assert!(bars(None, 8).iter().all(|bar| *bar > 0.0));
    }

    #[test]
    fn no_bar_is_invisible() {
        assert!(bars(Some(&[0, 0, 0]), 6).iter().all(|bar| *bar >= 0.08));
    }

    #[test]
    fn a_length_reads_as_a_clock() {
        assert_eq!(clock(Duration::from_secs(0)), "0:00");
        assert_eq!(clock(Duration::from_secs(9)), "0:09");
        assert_eq!(clock(Duration::from_secs(75)), "1:15");
        assert_eq!(clock(Duration::from_secs(3600)), "60:00");
    }
}
