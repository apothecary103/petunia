//! The shape of a sound, for the ones that arrive without one.
//!
//! Signal ships a waveform beside a voice note, which is why the bars can be
//! drawn without decoding anything — but only its own clients send one. A note
//! recorded elsewhere, an `.m4a` somebody attached, and everything petunia has
//! ever sent arrive with the field empty, and forty-four identical grey bars is
//! a picture of nothing standing where the sound should be.
//!
//! So the file is read. The result is the same array the protocol carries — one
//! byte a bar, loudest at 255 — so nothing downstream has to know which of the
//! two it got.

use std::path::Path;

use rodio::Decoder;

/// How many bars are kept. Finer than anything drawn, since the strip picks its
/// own count from how wide it ended up, and coarse enough that the array is a
/// couple of hundred bytes rather than a second copy of the audio.
const RESOLUTION: usize = 256;

/// Samples folded into one measurement while decoding. The count is not known in
/// advance for mp3, flac or ogg -- none of them stores one -- so the stream is
/// measured at a fixed rate and resampled down once the end is reached, which
/// costs a few thousand floats rather than the whole decoded stream.
const CHUNK: usize = 512;

/// What is worth decoding on the thread that is drawing.
///
/// Reading the shape means decoding the file, and this is asked for from a
/// render -- once per file, cached after that, but that once is a frame that
/// takes as long as the decode. A voice note is small (Signal's own run to a few
/// hundred kilobytes a minute), so the cap is set where a voice note stops and
/// an album track begins, and anything above it keeps the flat bar it had.
pub const LIMIT: u64 = 4 * 1024 * 1024;

/// The sound as bars, or nothing when the file cannot be read or is too large to
/// be worth reading here.
pub fn read(path: &Path) -> Option<Vec<u8>> {
    if std::fs::metadata(path).ok()?.len() > LIMIT {
        return None;
    }

    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let decoder = Decoder::new(file).ok()?;

    Some(shape(energies(decoder)))
}

/// How loud each chunk of the decoded stream is.
///
/// The root mean square rather than the loudest sample in it. A peak is what the
/// signal touched once; speech touches its peak in nearly every chunk it is not
/// silent in, so a strip of peaks is a strip of full-height bars with gaps —
/// which is what this drew before, and reads as a rendering fault rather than as
/// somebody talking. The mean is what the ear would call the volume.
fn energies(samples: impl Iterator<Item = f32>) -> Vec<f32> {
    let mut energies = Vec::new();
    let mut sum = 0.0f32;
    let mut counted = 0;

    for sample in samples {
        sum += sample * sample;
        counted += 1;
        if counted == CHUNK {
            energies.push((sum / CHUNK as f32).sqrt());
            sum = 0.0;
            counted = 0;
        }
    }
    if counted > 0 {
        energies.push((sum / counted as f32).sqrt());
    }
    energies
}

/// The energies as the bytes the protocol would have carried: resampled to a
/// fixed number of bars and scaled so the strip uses its whole height.
///
/// Scaled because a voice note is recorded at whatever level the room and the
/// phone agreed on, and drawn against an absolute scale a quiet one is a flat
/// line — the very thing this exists to avoid. Against the *loud end* rather
/// than the loudest bar, though: one cough, one knock on the microphone, and
/// everything that was actually said is drawn at a tenth of the height. So the
/// scale is set by `LOUD` of the way up the sorted bars and anything above it is
/// simply full — which is what a limiter does, and for the same reason.
pub fn shape(energies: Vec<f32>) -> Vec<u8> {
    /// Where the top of the strip is pinned in the sorted bars.
    const LOUD: f32 = 0.95;

    if energies.is_empty() {
        return Vec::new();
    }

    let bars: Vec<f32> = (0..RESOLUTION)
        .map(|bar| {
            let (from, to) = span(bar, RESOLUTION, energies.len());
            let range = &energies[from..to];
            range.iter().sum::<f32>() / range.len() as f32
        })
        .collect();

    let mut sorted = bars.clone();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let loudest = sorted[((sorted.len() - 1) as f32 * LOUD) as usize];
    if loudest <= 0.0 {
        return Vec::new();
    }

    bars.into_iter()
        .map(|bar| ((bar / loudest).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect()
}

/// Which of `len` values one of `wanted` bars is drawn from. Always at least one,
/// so a source shorter than the number of bars asked for repeats values rather
/// than reading an empty range.
pub fn span(bar: usize, wanted: usize, len: usize) -> (usize, usize) {
    let from = (bar * len / wanted.max(1)).min(len.saturating_sub(1));
    let to = ((bar + 1) * len).div_ceil(wanted.max(1)).clamp(from + 1, len);

    (from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loud_end_is_full_height() {
        let shape = shape(vec![0.1, 0.5, 1.0, 0.2]);

        assert_eq!(shape.len(), RESOLUTION);
        assert_eq!(shape.iter().copied().max(), Some(255));
    }

    /// A recording made at a whisper has to read as a waveform, not as a line.
    #[test]
    fn a_quiet_recording_still_fills_the_strip() {
        let quiet = shape(vec![0.001, 0.01, 0.004]);

        assert_eq!(quiet.iter().copied().max(), Some(255));
    }

    #[test]
    fn silence_has_no_shape() {
        assert!(shape(vec![0.0; 32]).is_empty());
        assert!(shape(Vec::new()).is_empty());
    }

    /// A steady tone reads as its own level, and a silent chunk as nothing.
    #[test]
    fn every_chunk_is_measured() {
        let samples = (0..CHUNK * 2).map(|at| match at < CHUNK {
            true => 0.5,
            false => 0.0,
        });

        assert_eq!(energies(samples), vec![0.5, 0.0]);
    }

    /// A tail shorter than a chunk is still sound that was in the file, and is
    /// measured against its own length rather than against a chunk it never
    /// filled.
    #[test]
    fn a_partial_chunk_is_kept() {
        assert_eq!(energies([0.25, 0.25].into_iter()), vec![0.25]);
    }

    /// One knock on the microphone must not shrink everything that was said.
    #[test]
    fn a_single_loud_moment_does_not_flatten_the_rest() {
        let mut spoken = vec![0.2; 200];
        spoken.push(1.0);

        let shape = shape(spoken);
        let speech = shape[0];

        assert!(speech > 200, "speech drawn at {speech} of 255");
    }

    /// Every bar reads from somewhere, and no bar reads past the end.
    #[test]
    fn a_span_is_never_empty_and_never_overruns() {
        for len in [1, 3, 44, 300] {
            for bar in 0..44 {
                let (from, to) = span(bar, 44, len);
                assert!(from < to, "{bar} of {len}: {from}..{to}");
                assert!(to <= len, "{bar} of {len}: {from}..{to}");
            }
        }
    }
}

