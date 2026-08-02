//! Playing faster without playing higher.
//!
//! rodio's own `set_speed` speeds a source up by lying about its sample rate,
//! which resamples: a voice note at 2× is the same recording an octave up, and
//! the person talking sounds like a cartoon. What a speed control is for is
//! getting through the words sooner, so the pitch has to stay where it was.
//!
//! WSOLA is the classical answer and the one every player uses. The signal is
//! cut into overlapping grains and the grains are laid back down at a different
//! spacing than they were taken at — closer together to shorten, further apart
//! to lengthen — so the waveform keeps its own periods and only the timeline
//! changes. Laid down blindly that is an echo, because two grains a hop apart
//! are rarely in phase with each other; the *WS* is the repair, a small search
//! around each grain's nominal position for the offset that best continues what
//! was already written. A pitch period is a few milliseconds and the search is
//! ten, so there is always one to find.
//!
//! Speed is shared rather than baked in, so it can change mid-note without the
//! source being rebuilt, and the stretcher publishes where it has reached *in
//! the file* — which rodio cannot, counting as it does in the timeline it has
//! stretched. That is what the playhead reads, and it is the same number
//! whatever the speed is doing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

/// How much of one grain overlaps the next, which is also how much output each
/// step writes. Long enough to hold several periods of a voice, short enough
/// that a syllable is not smeared across it.
const OVERLAP: Duration = Duration::from_millis(25);

/// How far either side of a grain's nominal position the search may look. Well
/// over one period of the lowest voice, which is what it has to be able to
/// slide by.
const SEARCH: Duration = Duration::from_millis(10);

/// How much of the overlap the search actually compares, and how coarsely.
///
/// A match good to four samples is a match good to a twelfth of a millisecond,
/// which is nothing beside the period it is lining up; comparing every sample
/// of every candidate would be a hundred times the arithmetic for a decision
/// that does not change.
const COMPARE: Duration = Duration::from_millis(12);
const STRIDE: usize = 4;

/// How fast to play, as a multiple, shared with whatever source is running.
///
/// A property of the player rather than of a file — it survives one voice note
/// ending and the next beginning — and read afresh on every grain, so changing
/// it while something is playing is a change to the next twenty-five
/// milliseconds and to nothing that has already been written.
#[derive(Clone)]
pub struct Speed(Arc<AtomicU32>);

impl Default for Speed {
    fn default() -> Self {
        Self(Arc::new(AtomicU32::new(1.0f32.to_bits())))
    }
}

impl Speed {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed)).clamp(0.25, 4.0)
    }

    pub fn set(&self, speed: f32) {
        self.0
            .store(speed.clamp(0.25, 4.0).to_bits(), Ordering::Relaxed);
    }
}

/// How far into the file the stretcher has read, in milliseconds.
///
/// One of these per file rather than one per player: a source that has been
/// cleared may still be dropping its last buffer, and a dead source writing to
/// a live playhead is the position jumping back a frame after every skip.
#[derive(Clone, Default)]
pub struct Progress(Arc<AtomicU64>);

impl Progress {
    pub fn position(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::Relaxed))
    }

    fn reached(&self, position: Duration) {
        self.0.store(position.as_millis() as u64, Ordering::Relaxed);
    }
}

pub struct Stretch<S> {
    input: S,
    channels: usize,
    rate: u32,
    speed: Speed,
    progress: Progress,

    /// Input read but not yet consumed, interleaved. `base` is the frame index
    /// of the first frame in it, so a frame's place in the buffer is arithmetic
    /// rather than a second cursor to keep in step.
    buffer: Vec<f32>,
    base: u64,
    ended: bool,

    /// Where the grain being faded out was taken from, and the frames of it
    /// still to fade. The next grain is chosen to continue *these*.
    grain: u64,
    tail: Vec<f32>,
    started: bool,

    ready: std::collections::VecDeque<f32>,

    overlap: usize,
    search: usize,
    compare: usize,
}

impl<S: Source> Stretch<S> {
    pub fn new(input: S, speed: Speed, progress: Progress) -> Self {
        let rate = input.sample_rate().get();
        let frames = |span: Duration| (rate as f64 * span.as_secs_f64()) as usize;

        Self {
            channels: input.channels().get() as usize,
            rate,
            overlap: frames(OVERLAP).max(1),
            search: frames(SEARCH),
            compare: frames(COMPARE).max(STRIDE),
            input,
            speed,
            progress,
            buffer: Vec::new(),
            base: 0,
            ended: false,
            grain: 0,
            tail: Vec::new(),
            started: false,
            ready: std::collections::VecDeque::new(),
        }
    }

    /// The frames held, which is the buffer measured in whole channel groups.
    fn held(&self) -> u64 {
        self.base + (self.buffer.len() / self.channels) as u64
    }

    /// Reads until the buffer covers everything before `upto`, or the file ends.
    fn ensure(&mut self, upto: u64) -> bool {
        while !self.ended && self.held() < upto {
            match self.input.next() {
                Some(sample) => self.buffer.push(sample),
                None => self.ended = true,
            }
        }
        self.held() >= upto
    }

    fn forget(&mut self, before: u64) {
        let frames = before.saturating_sub(self.base) as usize;
        let frames = frames.min(self.buffer.len() / self.channels);
        self.buffer.drain(..frames * self.channels);
        self.base += frames as u64;
    }

    fn at(&self, frame: u64) -> usize {
        (frame - self.base) as usize * self.channels
    }

    fn reached(&self, frame: u64) {
        self.progress
            .reached(Duration::from_secs_f64(frame as f64 / self.rate as f64));
    }

    /// The offset near `target` whose first overlap best continues the tail.
    ///
    /// Normalised by the candidate's own energy, so a loud passage does not win
    /// on volume alone — what is being asked is which offset is the same
    /// *shape*, not which is the largest.
    fn best(&self, target: u64, lowest: u64, highest: u64) -> u64 {
        let step = self.channels * STRIDE;
        let compared = self.compare / STRIDE;
        let mut best = (f32::MIN, target);

        let mut candidate = lowest;
        while candidate <= highest {
            let from = self.at(candidate);
            let (mut dot, mut energy) = (0.0f32, 0.0f32);
            for taken in 0..compared {
                let sample = self.buffer[from + taken * step];
                dot += sample * self.tail[taken * step];
                energy += sample * sample;
            }
            let score = dot / (energy.sqrt() + f32::EPSILON);
            if score > best.0 {
                best = (score, candidate);
            }
            candidate += STRIDE as u64;
        }
        best.1
    }

    /// Writes out the tail and everything the file has left behind it, which is
    /// where a run too short to search through ends up — the last fifty
    /// milliseconds of every file, and the whole of one shorter than that.
    /// `from` is the first frame nothing has written yet.
    fn drain(&mut self, from: u64) {
        self.ensure(u64::MAX);
        self.ready.extend(self.tail.drain(..));

        let end = self.held();
        if end > from {
            let at = self.at(from.max(self.base));
            self.ready.extend(self.buffer[at..].iter().copied());
        }
        self.buffer.clear();
        self.base = end;
        self.grain = end;
        self.reached(end);
    }

    fn produce(&mut self) {
        let overlap = self.overlap as u64;

        if !self.started {
            self.started = true;
            if !self.ensure(self.grain + 2 * overlap) {
                self.drain(self.grain);
                return;
            }
            let from = self.at(self.grain);
            let (head, tail) = (from, from + self.overlap * self.channels);
            self.ready.extend(self.buffer[head..tail].iter().copied());
            self.tail = self.buffer[tail..tail + self.overlap * self.channels].to_vec();
            self.reached(self.grain + overlap);
            return;
        }
        if self.tail.is_empty() {
            return;
        }

        let hop = (self.overlap as f32 * self.speed.get()).round().max(1.0) as u64;
        let target = self.grain + hop;
        let lowest = target.saturating_sub(self.search as u64).max(self.base);
        let highest = target + self.search as u64;

        // Everything the search may read, which is the furthest candidate plus
        // the two overlaps a grain is made of.
        if !self.ensure(highest + 2 * overlap) {
            self.drain(self.grain + 2 * overlap);
            return;
        }
        self.forget(lowest);

        let chosen = self.best(target, lowest, highest);
        let from = self.at(chosen);
        let count = self.overlap * self.channels;

        // Linear and complementary, so two grains that are already the same
        // reconstruct exactly — which is what the search finds at a speed of
        // one, and is why playing at 1× is the recording rather than a
        // resynthesis of it.
        for taken in 0..count {
            let fade = (taken / self.channels) as f32 / self.overlap as f32;
            let out = self.tail[taken] * (1.0 - fade) + self.buffer[from + taken] * fade;
            self.ready.push_back(out);
        }

        self.tail
            .copy_from_slice(&self.buffer[from + count..from + 2 * count]);
        self.grain = chosen;
        self.reached(chosen + overlap);
    }
}

impl<S: Source> Iterator for Stretch<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(sample) = self.ready.pop_front() {
            return Some(sample);
        }
        self.produce();
        self.ready.pop_front()
    }
}

impl<S: Source> Source for Stretch<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.input.channels()
    }

    /// Unchanged, which is the whole point: a grain keeps the periods it was cut
    /// from, so the samples leaving here are the same pitch at the same rate and
    /// only fewer of them.
    fn sample_rate(&self) -> rodio::SampleRate {
        self.input.sample_rate()
    }

    /// The file's own length. What plays is shorter, but the bar, the clock and
    /// the playhead are all in the recording's time, so this is the one that
    /// keeps them agreeing with each other.
    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.input.try_seek(position)?;

        let frame = (position.as_secs_f64() * self.rate as f64) as u64;
        self.buffer.clear();
        self.ready.clear();
        self.tail.clear();
        self.base = frame;
        self.grain = frame;
        self.ended = false;
        self.started = false;
        self.reached(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    const RATE: u32 = 8_000;

    fn tone(seconds: f32, hertz: f32) -> SamplesBuffer {
        let count = (RATE as f32 * seconds) as usize;
        let samples: Vec<f32> = (0..count)
            .map(|at| {
                (at as f32 / RATE as f32 * hertz * std::f32::consts::TAU).sin()
            })
            .collect();
        SamplesBuffer::new(
            std::num::NonZeroU16::new(1).unwrap(),
            std::num::NonZeroU32::new(RATE).unwrap(),
            samples,
        )
    }

    fn played(speed: f32, seconds: f32) -> Vec<f32> {
        let control = Speed::default();
        control.set(speed);
        Stretch::new(tone(seconds, 220.0), control, Progress::default()).collect()
    }

    /// The rate is what a resampling speed control changes, and the one thing
    /// this must not: a higher rate on the same samples *is* the pitch shift.
    #[test]
    fn the_sample_rate_never_moves() {
        let stretch = Stretch::new(tone(1.0, 220.0), Speed::default(), Progress::default());

        assert_eq!(stretch.sample_rate().get(), RATE);
        assert_eq!(stretch.channels().get(), 1);
    }

    #[test]
    fn twice_as_fast_is_half_as_long() {
        let once = played(1.0, 2.0).len() as f32;
        let twice = played(2.0, 2.0).len() as f32;

        assert!((twice / once - 0.5).abs() < 0.1, "{twice} against {once}");
    }

    /// A grain laid down where it was taken from is the recording, so 1× has to
    /// be a pass-through rather than a resynthesis that merely sounds like one.
    #[test]
    fn playing_at_one_is_the_recording() {
        let original: Vec<f32> = tone(0.5, 220.0).collect();
        let through = played(1.0, 0.5);

        let compared = through.len().min(original.len()) - RATE as usize / 20;
        assert!(compared > 0);
        for at in 0..compared {
            assert!(
                (through[at] - original[at]).abs() < 1e-4,
                "sample {at}: {} against {}",
                through[at],
                original[at]
            );
        }
    }

    /// Faster has to stay the same note. Counting zero crossings is the cheapest
    /// way to ask what pitch came out, and it is unambiguous for one sine.
    #[test]
    fn the_pitch_survives_the_speed() {
        let crossings = |samples: &[f32]| {
            samples
                .windows(2)
                .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
                .count() as f32
        };

        let slow = played(1.0, 2.0);
        let fast = played(2.0, 2.0);

        let (slow, fast) = (
            crossings(&slow) / slow.len() as f32,
            crossings(&fast) / fast.len() as f32,
        );
        assert!((fast / slow - 1.0).abs() < 0.05, "{fast} against {slow}");
    }

    /// A sound shorter than the grain machinery needs still has to come out,
    /// rather than being swallowed by a search that could never run.
    #[test]
    fn something_too_short_to_stretch_still_plays() {
        let control = Speed::default();
        control.set(2.0);
        let played: Vec<f32> =
            Stretch::new(tone(0.01, 220.0), control, Progress::default()).collect();

        assert_eq!(played.len(), (RATE as f32 * 0.01) as usize);
    }

    #[test]
    fn the_playhead_counts_in_the_file_and_not_in_the_playback() {
        let progress = Progress::default();
        let control = Speed::default();
        control.set(2.0);
        let _: Vec<f32> = Stretch::new(tone(2.0, 220.0), control, progress.clone()).collect();

        let reached = progress.position().as_secs_f32();
        assert!((reached - 2.0).abs() < 0.1, "reached {reached}");
    }

    #[test]
    fn a_speed_is_kept_within_reason() {
        let control = Speed::default();

        control.set(1000.0);
        assert_eq!(control.get(), 4.0);
        control.set(0.0);
        assert_eq!(control.get(), 0.25);
    }
}
