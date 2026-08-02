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
    /// Jumps to a fraction of the way through this file, starting it if it is
    /// not the one loaded. The path travels with the request rather than being
    /// checked against the published snapshot by the caller: that snapshot is
    /// republished on a tick, so a click on the bar within `TICK` of pressing
    /// play read the state from before the press and toggled it back off.
    Seek(PathBuf, f32),
    /// How fast to play, as a multiple. Applies to whatever is playing and to
    /// whatever plays next.
    Speed(f32),
    /// A short tone. Its own voice on the mixer, so it lands over whatever is
    /// playing rather than stopping it.
    Chime(Chime),
    Stop,
}

/// The two moments worth a sound. Sent is the shorter and the higher of the
/// two — it confirms something you did, and a confirmation that lingers is a
/// confirmation you notice; received is the one that has to carry across a
/// room, so it is two notes rather than one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chime {
    Sent,
    Received,
}

/// The speeds a voice note is offered at, cycled through by one control.
/// Signal's own ladder, and short enough that the pill is a fixed width.
pub const SPEEDS: [f32; 3] = [1.0, 1.5, 2.0];

/// The next speed on the ladder, wrapping.
pub fn next_speed(speed: f32) -> f32 {
    let at = SPEEDS
        .iter()
        .position(|candidate| (candidate - speed).abs() < 0.01)
        .unwrap_or(0);
    SPEEDS[(at + 1) % SPEEDS.len()]
}

/// A multiple as a label. Trailing zeroes dropped, so 1.5 reads as `1.5×` and
/// 2.0 as `2×` rather than `2.0×`.
pub fn speed_label(speed: f32) -> String {
    if (speed - speed.round()).abs() < 0.01 {
        format!("{}×", speed.round() as i32)
    } else {
        format!("{speed}×")
    }
}

/// What is playing and how far in. Cloned out per frame, so the views never
/// hold the lock.
///
/// The times are the file's own, whatever speed it is being played at: rodio
/// counts in the sped-up timeline, and a bar that ran out halfway through a
/// track at double speed would be reporting the player's clock rather than the
/// recording's. The conversion is `serve`'s, at the one boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub file: Option<PathBuf>,
    pub playing: bool,
    pub position: Duration,
    /// Absent for a format whose decoder cannot say, which is common enough for
    /// Signal's voice notes that the bar has to cope.
    pub duration: Option<Duration>,
    /// How fast, as a multiple. A property of the player rather than of a file,
    /// so it survives one voice note ending and the next beginning.
    pub speed: f32,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            file: None,
            playing: false,
            position: Duration::ZERO,
            duration: None,
            speed: 1.0,
        }
    }
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

    pub fn seek(&self, file: PathBuf, fraction: f32) {
        let _ = self.requests.send(Request::Seek(file, fraction));
    }

    pub fn set_speed(&self, speed: f32) {
        let _ = self.requests.send(Request::Speed(speed));
    }

    pub fn chime(&self, chime: Chime) {
        let _ = self.requests.send(Request::Chime(chime));
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
    // A voice of its own on the same mixer. A chime through the player that is
    // holding a voice note would have to clear it first, which is a sound that
    // stops what you were listening to in order to tell you something arrived.
    let bell = rodio::Player::connect_new(sink.mixer());
    let mut state = Playback::default();

    loop {
        match requests.recv_timeout(TICK) {
            Ok(Request::Toggle(file)) => toggle(&player, &mut state, file),
            Ok(Request::Seek(file, fraction)) => seek(&player, &mut state, file, fraction),
            Ok(Request::Speed(speed)) => set_speed(&player, &mut state, speed),
            Ok(Request::Chime(chime)) => {
                bell.append(tone(chime));
                bell.play();
            }
            Ok(Request::Stop) => {
                player.clear();
                state = Playback {
                    speed: state.speed,
                    ..Default::default()
                };
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        if state.file.is_some() {
            state.position = elapsed(&player, state.speed);
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

    load(player, state, file);
}

/// Reads a file in and starts it. The player's speed survives, being the
/// player's rather than the file's.
fn load(player: &rodio::Player, state: &mut Playback, file: PathBuf) {
    let speed = state.speed;
    player.clear();
    // `Decoder::try_from` rather than `Decoder::new`, which is the whole of why
    // seeking did nothing: `new` builds a decoder that is neither seekable nor
    // told how many bytes it has, so `try_seek` refuses outright and symphonia
    // cannot work out the length of a format that does not store one — which is
    // mp3, ogg and every voice note Signal sends. `try_from` reads the length
    // off the file's own metadata and marks the stream seekable.
    let source = match std::fs::File::open(&file)
        .map_err(Error::Io)
        .and_then(|file| Decoder::try_from(file).map_err(Error::Decode))
    {
        Ok(source) => source,
        Err(error) => {
            warn!(%error, file = %file.display(), "cannot play this file");
            *state = Playback {
                speed,
                ..Default::default()
            };
            return;
        }
    };

    // The tags are the fallback for a stream the decoder still cannot measure,
    // having read the container's own properties rather than the codec's.
    let duration = source
        .total_duration()
        .or_else(|| crate::song::read(&file)?.duration);

    *state = Playback {
        duration,
        speed,
        file: Some(file),
        playing: true,
        position: Duration::ZERO,
    };
    player.append(source);
    player.set_speed(speed);
    player.play();
}

/// Clicking into the bar of something that is not playing means "play this,
/// from here" — and so does clicking into one that has run to the end, where
/// the queue is empty and a seek would land on nothing.
fn seek(player: &rodio::Player, state: &mut Playback, file: PathBuf, fraction: f32) {
    if !state.is(&file) || player.empty() {
        load(player, state, file);
    }

    let Some(duration) = state.duration else {
        return;
    };
    let target = duration.mul_f32(fraction.clamp(0.0, 1.0));
    if let Err(error) = player.try_seek(scaled(target, state.speed)) {
        warn!(%error, "cannot seek in this file");
        return;
    }
    // Published straight away rather than waited a tick for, so the playhead
    // lands under the pointer that put it there.
    state.position = target;
}

/// rodio speeds a source up by reporting a higher sample rate, and counts the
/// position it gives back in that same stretched timeline — so changing the
/// speed rescales every sample already counted and the playhead jumps. Seeking
/// back to where it was resets the count against the new rate.
fn set_speed(player: &rodio::Player, state: &mut Playback, speed: f32) {
    let speed = speed.clamp(0.25, 4.0);
    let at = state.position;
    state.speed = speed;
    player.set_speed(speed);

    if state.file.is_some()
        && !player.empty()
        && let Err(error) = player.try_seek(scaled(at, speed))
    {
        warn!(%error, "cannot hold the position across a speed change");
    }
}

/// The rate the chimes are built at. Any rate would do — the mixer resamples —
/// and this is the one every output device on a desktop is already running at.
const CHIME_RATE: u32 = 48_000;

/// A chime, synthesised rather than shipped.
///
/// A pair of WAV files would be two more assets to carry, two more things to
/// license, and two more decisions somebody would have to make about what a
/// chat application should sound like. Two sine partials under an exponential
/// decay is a struck bell, which is what a notification has sounded like since
/// there were notifications, and it is thirty lines.
///
/// Deliberately quiet. This plays every time a message lands, which for a busy
/// group is a great many times, and the sound that survives that is the one you
/// stop hearing rather than the one you turn off.
fn tone(chime: Chime) -> rodio::buffer::SamplesBuffer {
    /// The notes, in hertz, and how long each is held.
    const SENT: &[(f32, f32)] = &[(880.0, 0.10)];
    const RECEIVED: &[(f32, f32)] = &[(660.0, 0.09), (990.0, 0.16)];
    const LEVEL: f32 = 0.18;

    let notes = match chime {
        Chime::Sent => SENT,
        Chime::Received => RECEIVED,
    };

    let mut samples: Vec<f32> = Vec::new();
    for (frequency, seconds) in notes {
        let count = (CHIME_RATE as f32 * seconds) as usize;
        for at in 0..count {
            let time = at as f32 / CHIME_RATE as f32;
            // A struck note: all of its energy at the start, decaying away.
            // Squared over the first millisecond as well, so the attack does
            // not begin on a step — a discontinuity at nought is a click.
            let decay = (-time * 18.0).exp() * (time * 400.0).min(1.0);
            let fundamental = (time * frequency * std::f32::consts::TAU).sin();
            // The octave above, quietly. One sine is a test tone; two are an
            // instrument.
            let partial = (time * frequency * 2.0 * std::f32::consts::TAU).sin() * 0.3;
            samples.push((fundamental + partial) * decay * LEVEL);
        }
    }

    rodio::buffer::SamplesBuffer::new(
        std::num::NonZeroU16::new(1).expect("one channel"),
        std::num::NonZeroU32::new(CHIME_RATE).expect("a rate"),
        samples,
    )
}

/// A time in the file, as the player counts it.
fn scaled(position: Duration, speed: f32) -> Duration {
    position.div_f32(speed.max(0.01))
}

/// Where the player has got to, in the file's own time.
fn elapsed(player: &rodio::Player, speed: f32) -> Duration {
    player.get_pos().mul_f32(speed)
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
    fn the_speed_ladder_wraps() {
        assert_eq!(next_speed(1.0), 1.5);
        assert_eq!(next_speed(1.5), 2.0);
        assert_eq!(next_speed(2.0), 1.0);
    }

    /// A speed nothing set — a file played before the control existed — still
    /// has to name a next one rather than fall off the ladder.
    #[test]
    fn an_unknown_speed_starts_the_ladder_again() {
        assert_eq!(next_speed(3.7), 1.5);
    }

    #[test]
    fn a_speed_reads_without_a_trailing_zero() {
        assert_eq!(speed_label(1.0), "1×");
        assert_eq!(speed_label(1.5), "1.5×");
        assert_eq!(speed_label(2.0), "2×");
    }

    #[test]
    fn a_length_reads_as_a_clock() {
        assert_eq!(clock(Duration::from_secs(0)), "0:00");
        assert_eq!(clock(Duration::from_secs(9)), "0:09");
        assert_eq!(clock(Duration::from_secs(75)), "1:15");
        assert_eq!(clock(Duration::from_secs(3600)), "60:00");
    }
}
