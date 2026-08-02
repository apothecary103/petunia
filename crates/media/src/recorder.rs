//! Recording a voice note.
//!
//! The microphone is owned by a thread of its own, the way the output device
//! is: a `cpal` stream is not `Send`, and the window must never be the thing
//! waiting on a buffer. The view asks through two flags and reads a shared
//! snapshot for the meter.
//!
//! **Quality.** Signal's own clients send a voice note as roughly 32 kbit of
//! AAC — enough for a sentence in a quiet room and audibly not enough for
//! anything else. Petunia writes the microphone out as it arrived: sixteen-bit
//! PCM at the device's own rate, in a WAV. It is a megabyte a minute against
//! Signal's quarter of one, and it is the recording rather than a guess at it.
//!
//! Sixteen-bit PCM specifically, and not the thirty-two-bit float rodio's own
//! writer emits: what is on the other end of this is a phone, and a phone's
//! decoder knows the format every recorder has written since 1991.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use rodio::Source;
use tracing::warn;

/// How many samples the level and the waveform are gathered over. At 48 kHz
/// that is about a fiftieth of a second, which is fine enough for a meter that
/// reads as live and coarse enough that the vector stays small over an hour.
const CHUNK: usize = 1024;


/// What is being recorded, as the window sees it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recording {
    pub active: bool,
    pub elapsed: Duration,
    /// The loudest sample of the last chunk, 0 to 1. What the meter moves on.
    pub level: f32,
    /// The shape so far, one byte a bar, in the same units a received waveform
    /// arrives in — so the same code draws both.
    pub shape: Vec<u8>,
    /// Set when the microphone could not be opened at all, which is a thing to
    /// say rather than a control that does nothing.
    pub failed: bool,
}

/// A finished recording.
pub struct Recorded {
    pub path: PathBuf,
    pub duration: Duration,
    pub waveform: Vec<u8>,
}

pub struct Recorder {
    stop: Arc<AtomicBool>,
    state: Arc<RwLock<Recording>>,
    worker: Option<std::thread::JoinHandle<Option<Recorded>>>,
    path: PathBuf,
}

impl Recorder {
    /// Opens the microphone and starts writing. The file is created here rather
    /// than at the end: a recording that only exists in memory is a recording
    /// lost to a crash, and there is nothing to gain by holding it.
    pub fn start(into: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(RwLock::new(Recording {
            active: true,
            ..Default::default()
        }));

        let worker = std::thread::Builder::new()
            .name("petunia-recorder".into())
            .spawn({
                let (stop, state, path) = (stop.clone(), state.clone(), into.clone());
                move || capture(&path, &stop, &state)
            })
            .ok();

        Self {
            stop,
            state,
            worker,
            path: into,
        }
    }

    pub fn state(&self) -> Recording {
        self.state.read().map(|state| state.clone()).unwrap_or_default()
    }

    /// Stops and hands back what was recorded. Joins, which is a wait of one
    /// chunk — the loop checks the flag between buffers, and a buffer is
    /// twenty milliseconds.
    pub fn finish(mut self) -> Option<Recorded> {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().and_then(|worker| worker.join().ok()).flatten()
    }

    /// Stops and throws it away, file and all.
    pub fn cancel(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // A recorder dropped without being finished is a window that went away
        // mid-recording. Stop the thread; the file is a temporary one.
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn capture(path: &Path, stop: &AtomicBool, published: &RwLock<Recording>) -> Option<Recorded> {
    let microphone = match open() {
        Some(microphone) => microphone,
        None => {
            if let Ok(mut published) = published.write() {
                published.active = false;
                published.failed = true;
            }
            return None;
        }
    };

    let rate = microphone.sample_rate().get();
    let channels = microphone.channels().get();
    let format = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = match hound::WavWriter::create(path, format) {
        Ok(writer) => writer,
        Err(error) => {
            warn!(%error, path = %path.display(), "cannot write the recording");
            if let Ok(mut published) = published.write() {
                published.active = false;
                published.failed = true;
            }
            return None;
        }
    };

    // The mean per chunk, which is what `waveform::shape` reads a stored file
    // as — so a note recorded here and one received from a phone are drawn from
    // the same measurement rather than from two that happen to look similar.
    // The peak is kept beside it for the meter, which is a different question:
    // a meter answers "is it hearing me", and that is about the loudest thing
    // it heard, not the average.
    let mut energies: Vec<f32> = Vec::new();
    let mut written: u64 = 0;
    let mut chunk = 0usize;
    let mut sum = 0.0f32;
    let mut loudest = 0.0f32;

    for sample in microphone {
        if writer.write_sample(quantised(sample)).is_err() {
            break;
        }
        written += 1;
        sum += sample * sample;
        loudest = loudest.max(sample.abs());
        chunk += 1;

        if chunk >= CHUNK {
            energies.push((sum / CHUNK as f32).sqrt());
            let elapsed = seconds(written, rate, channels);
            if let Ok(mut published) = published.write() {
                published.elapsed = elapsed;
                published.level = loudest;
                published.shape = crate::waveform::shape(energies.clone());
            }
            (chunk, sum, loudest) = (0, 0.0, 0.0);
        }

        if stop.load(Ordering::Relaxed) {
            break;
        }
    }

    let duration = seconds(written, rate, channels);
    if let Ok(mut published) = published.write() {
        published.active = false;
        published.elapsed = duration;
        published.level = 0.0;
    }

    if let Err(error) = writer.finalize() {
        warn!(%error, "could not close the recording");
        return None;
    }

    Some(Recorded {
        path: path.to_path_buf(),
        duration,
        waveform: crate::waveform::shape(energies),
    })
}

fn open() -> Option<rodio::microphone::Microphone> {
    let builder = rodio::microphone::MicrophoneBuilder::new();
    let configured = builder
        .default_device()
        .and_then(|builder| builder.default_config());

    match configured {
        Ok(builder) => match builder.open_stream() {
            Ok(microphone) => Some(microphone),
            Err(error) => {
                warn!(%error, "cannot open the microphone");
                None
            }
        },
        Err(error) => {
            warn!(%error, "no microphone to record with");
            None
        }
    }
}

/// A float sample as the sixteen-bit integer a WAV holds. Clamped rather than
/// wrapped: a sample over one is a clipped sample, and wrapping turns the
/// loudest moment of a recording into a crack of white noise.
fn quantised(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

fn seconds(samples: u64, rate: u32, channels: u16) -> Duration {
    if rate == 0 {
        return Duration::ZERO;
    }
    let frames = samples / u64::from(channels).max(1);
    Duration::from_secs_f64(frames as f64 / f64::from(rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_over_one_clips_rather_than_wraps() {
        assert_eq!(quantised(2.0), i16::MAX);
        assert_eq!(quantised(-2.0), -i16::MAX);
        assert_eq!(quantised(0.0), 0);
    }

    #[test]
    fn a_length_counts_frames_rather_than_samples() {
        // One second of stereo at 48 kHz is ninety-six thousand samples.
        assert_eq!(seconds(96_000, 48_000, 2), Duration::from_secs(1));
        assert_eq!(seconds(48_000, 48_000, 1), Duration::from_secs(1));
    }

    /// Nothing recorded is a length of nothing rather than a division by zero.
    #[test]
    fn an_empty_recording_has_no_length() {
        assert_eq!(seconds(0, 48_000, 1), Duration::ZERO);
        assert_eq!(seconds(10, 0, 0), Duration::ZERO);
    }
}
