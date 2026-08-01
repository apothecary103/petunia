//! What an audio file says about itself.
//!
//! Signal has one kind of attached sound and draws it one way: a play control, a
//! waveform and a clock. That is right for the thing it was invented for, which
//! is somebody talking, and wrong for a record — an album track arrives with its
//! title, its artist, its cover and the numbers that say what was kept of it, and
//! a row of grey bars reports none of that while implying it was recorded on a
//! phone.
//!
//! So the file is asked. A voice note is not: Signal marks one, and a mark from
//! the sender outranks anything guessed from the bytes.

use std::path::Path;
use std::time::Duration;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::ItemKey;
use lofty::probe::Probe;

/// A record, as the file describes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Song {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// What was kept of it. Both are optional and often only one is known: a lossy
    /// format has a sampling rate and no meaningful depth at all.
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub duration: Option<Duration>,
    /// Whether a picture is embedded. The bytes are read separately, where there
    /// is somewhere to put them.
    pub cover: bool,
}

impl Song {
    /// Whether this is worth drawing as a record rather than as a sound.
    ///
    /// A title alone is not enough: every phone recording carries a filename in
    /// one, and a card with a single line on it says less than the waveform it
    /// replaced. Somebody has to be named — an artist or an album — because that
    /// is what distinguishes a track from an audio file.
    pub fn is_a_record(&self) -> bool {
        self.title.is_some() && (self.artist.is_some() || self.album.is_some())
    }

    /// The line under the names: the format's own numbers, in the order a record
    /// shop prints them and with the units nobody has to expand.
    pub fn quality(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(depth) = self.bit_depth {
            parts.push(format!("{depth}-bit"));
        }
        if let Some(rate) = self.sample_rate {
            // Two decimals would be 44.10, and one loses nothing: the rates in
            // use are 44.1, 48, 88.2, 96, 176.4 and 192.
            let khz = rate as f32 / 1000.0;
            parts.push(match khz.fract() == 0.0 {
                true => format!("{khz:.0} kHz"),
                false => format!("{khz:.1} kHz"),
            });
        }
        match self.channels {
            Some(1) => parts.push("Mono".to_owned()),
            Some(2) => parts.push("Stereo".to_owned()),
            Some(more) if more > 2 => parts.push(format!("{more} channels")),
            _ => {}
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// Reads the tags and the stream's own parameters. `None` for anything that is
/// not audio, or that will not parse -- a file petunia cannot read is a file the
/// waveform still draws.
pub fn read(path: &Path) -> Option<Song> {
    let tagged = Probe::open(path).ok()?.read().ok()?;
    let properties = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let text = |key: ItemKey| {
        tag.and_then(|tag| tag.get_string(key))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };

    Some(Song {
        title: text(ItemKey::TrackTitle),
        artist: text(ItemKey::TrackArtist).or_else(|| text(ItemKey::AlbumArtist)),
        album: text(ItemKey::AlbumTitle),
        sample_rate: properties.sample_rate(),
        bit_depth: properties.bit_depth(),
        channels: properties.channels(),
        duration: Some(properties.duration()).filter(|duration| !duration.is_zero()),
        cover: tag.is_some_and(|tag| tag.picture_count() > 0),
    })
}

/// The embedded cover, as the bytes it is stored as. Whatever picture format that
/// happens to be is the decoder's problem, not this one's.
pub fn cover(path: &Path) -> Option<Vec<u8>> {
    let tagged = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.pictures()
        .first()
        .map(|picture| picture.data().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        Song {
            title: Some("Teardrop".into()),
            artist: Some("Massive Attack".into()),
            ..Default::default()
        }
    }

    /// A phone recording carries a filename in the title tag and nothing else, so
    /// a title on its own must not turn a voice memo into an album track.
    #[test]
    fn a_title_alone_is_not_a_record() {
        let named = Song {
            title: Some("audio-2026-08-01.m4a".into()),
            ..Default::default()
        };

        assert!(!named.is_a_record());
        assert!(song().is_a_record());
    }

    #[test]
    fn an_album_is_enough_without_an_artist() {
        let album = Song {
            title: Some("Teardrop".into()),
            album: Some("Mezzanine".into()),
            ..Default::default()
        };

        assert!(album.is_a_record());
    }

    #[test]
    fn the_quality_line_reads_as_a_record_shop_prints_it() {
        let studio = Song {
            bit_depth: Some(24),
            sample_rate: Some(96_000),
            channels: Some(2),
            ..song()
        };

        assert_eq!(studio.quality().as_deref(), Some("24-bit · 96 kHz · Stereo"));
    }

    /// The one rate that is not a whole number of kilohertz, and the one everybody
    /// has the most of.
    #[test]
    fn forty_four_one_keeps_its_decimal() {
        let cd = Song {
            sample_rate: Some(44_100),
            ..Default::default()
        };

        assert_eq!(cd.quality().as_deref(), Some("44.1 kHz"));
    }

    /// A lossy file knows its rate and not its depth, and the line has to read
    /// without the part that is missing rather than print a blank.
    #[test]
    fn what_is_unknown_is_left_out() {
        let lossy = Song {
            sample_rate: Some(48_000),
            channels: Some(1),
            ..Default::default()
        };

        assert_eq!(lossy.quality().as_deref(), Some("48 kHz · Mono"));
        assert_eq!(Song::default().quality(), None);
    }

    #[test]
    fn a_file_that_is_not_audio_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "not a song").unwrap();

        assert!(read(&path).is_none());
        assert!(cover(&path).is_none());
    }
}
