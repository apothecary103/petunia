//! Playing video in the window.
//!
//! There is no video pipeline in gpui, but there is a `surface` element on macOS
//! that draws a CoreVideo pixel buffer — which is exactly what AVFoundation
//! hands out. So a player here is an `AVPlayer` with a video output attached: it
//! decodes and plays the sound itself, and each frame we pull the current pixel
//! buffer and draw it.
//!
//! On every other platform this is a stub and video is handed to the system.

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;
    use std::time::Duration;

    use core_foundation::base::TCFType;
    use objc2::AnyThread;
use objc2::rc::Retained;
    use objc2_av_foundation::{AVPlayer, AVPlayerItem, AVPlayerItemVideoOutput};
    use objc2_core_media::CMTime;
    use objc2_foundation::{
        MainThreadMarker, NSDictionary, NSNumber, NSString, NSURL,
    };

    /// What the pixel buffers come out as. BGRA is what gpui's atlas and its
    /// surface shader both expect, so asking for it here avoids a conversion
    /// per frame.
    const BGRA: u32 = 0x42475241;

    pub struct Player {
        player: Retained<AVPlayer>,
        item: Retained<AVPlayerItem>,
        output: Retained<AVPlayerItemVideoOutput>,
        /// Kept so a frame remains available while paused, and while the decoder
        /// has nothing newer to give.
        frame: Option<core_video::pixel_buffer::CVPixelBuffer>,
    }

    impl Player {
        /// Opens a file, paused at its first frame.
        ///
        /// Returns `None` off the main thread, and when AVFoundation cannot make
        /// sense of the file — a codec macOS does not have, most likely, which
        /// is what the caller's hand-off to the system is for.
        pub fn open(path: &Path) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;

            // Safety: every one of these is an ordinary AVFoundation call on the
            // main thread, with objects this struct owns for as long as it uses
            // them.
            unsafe {
                let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
                let item = AVPlayerItem::playerItemWithURL(&url, mtm);

                let key = NSString::from_str("PixelFormatType");
                let format = NSNumber::new_u32(BGRA);
                let attributes = NSDictionary::from_slices(
                    &[&*key],
                    &[format.as_ref() as &objc2::runtime::AnyObject],
                );
                let output = AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                    AVPlayerItemVideoOutput::alloc(),
                    Some(&attributes),
                );
                item.addOutput(&output);

                let player = AVPlayer::playerWithPlayerItem(Some(&item), mtm);

                Some(Self {
                    player,
                    item,
                    output,
                    frame: None,
                })
            }
        }

        pub fn play(&self) {
            unsafe { self.player.play() };
        }

        pub fn pause(&self) {
            unsafe { self.player.pause() };
        }

        pub fn is_playing(&self) -> bool {
            unsafe { self.player.rate() != 0.0 }
        }

        /// The video's own pixel dimensions, once the item has loaded enough to
        /// say. Zero until then, which is not a size.
        pub fn size(&self) -> Option<(f32, f32)> {
            let size = unsafe { self.item.presentationSize() };
            let (width, height) = (size.width as f32, size.height as f32);
            (width > 0.0 && height > 0.0).then_some((width, height))
        }

        pub fn toggle(&self) {
            if self.is_playing() {
                self.pause();
            } else {
                self.play();
            }
        }

        pub fn position(&self) -> Duration {
            seconds(unsafe { self.player.currentTime() })
        }

        pub fn duration(&self) -> Option<Duration> {
            let duration = seconds(unsafe { self.item.duration() });
            (duration > Duration::ZERO).then_some(duration)
        }

        pub fn seek(&self, fraction: f32) {
            let Some(duration) = self.duration() else {
                return;
            };
            let target = duration.as_secs_f64() * f64::from(fraction.clamp(0.0, 1.0));
            unsafe {
                // A tolerance of zero would make every drag of the bar wait for
                // the next keyframe search; the default is what a scrubber wants.
                self.player.seekToTime(CMTime {
                    value: (target * 600.0) as i64,
                    timescale: 600,
                    flags: objc2_core_media::CMTimeFlags::Valid,
                    epoch: 0,
                });
            }
        }

        /// The frame for right now, or the last one if the decoder has nothing
        /// newer. Called once per drawn frame.
        pub fn frame(&mut self) -> Option<core_video::pixel_buffer::CVPixelBuffer> {
            unsafe {
                let at = self.player.currentTime();
                if self.output.hasNewPixelBufferForItemTime(at)
                    && let Some(buffer) = self
                        .output
                        .copyPixelBufferForItemTime_itemTimeForDisplay(at, std::ptr::null_mut())
                {
                    // The two crates wrap the same CoreVideo object; gpui's
                    // surface element takes the `core-video` one, and
                    // AVFoundation's bindings produce the `objc2` one. Retained
                    // here so both halves own a reference.
                    let raw = Retained::into_raw(buffer) as core_video::pixel_buffer::CVPixelBufferRef;
                    self.frame = Some(
                        core_video::pixel_buffer::CVPixelBuffer::wrap_under_create_rule(raw),
                    );
                }
            }
            self.frame.clone()
        }

        /// Whether playback has run off the end, so the control can go back to
        /// showing a play arrow.
        pub fn finished(&self) -> bool {
            match self.duration() {
                Some(duration) => self.position() >= duration,
                None => false,
            }
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            self.pause();
        }
    }

    /// The frame a video is shown as before anyone plays it, as PNG bytes.
    ///
    /// Taken a moment in rather than at zero: the first frame of a phone
    /// recording is very often black, and a black rectangle is no better than
    /// the placeholder it replaces. Runs off the main thread, which
    /// `AVAssetImageGenerator` permits and `AVPlayer` does not.
    pub fn poster(path: &Path) -> Option<Vec<u8>> {
        use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};

        unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
            generator.setAppliesPreferredTrackTransform(true);

            let at = CMTime {
                value: 600,
                timescale: 600,
                flags: objc2_core_media::CMTimeFlags::Valid,
                epoch: 0,
            };
            // The asynchronous replacement takes a completion block, and this
            // already runs on a thread of its own where blocking is the point.
            #[allow(deprecated)]
            let image = generator
                .copyCGImageAtTime_actualTime_error(at, std::ptr::null_mut())
                .ok()?;

            encode(&image)
        }
    }

    /// A `CGImage` as PNG bytes, so the cache stores one kind of thing and the
    /// renderer needs no second decoding path.
    fn encode(image: &objc2_core_graphics::CGImage) -> Option<Vec<u8>> {
        use objc2_core_foundation::{CFData, CFMutableData};
        use objc2_image_io::CGImageDestination;

        unsafe {
            let data = CFMutableData::new(None, 0)?;
            let kind = objc2_core_foundation::CFString::from_str("public.png");
            let destination = CGImageDestination::with_data(&data, &kind, 1, None)?;
            CGImageDestination::add_image(&destination, image, None);
            if !CGImageDestination::finalize(&destination) {
                return None;
            }
            Some(CFData::to_vec(&data))
        }
    }

    fn seconds(time: CMTime) -> Duration {
        if time.timescale <= 0 || time.value < 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(time.value as f64 / f64::from(time.timescale))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;
    use std::time::Duration;

    /// No in-process video outside macOS: gpui only has a surface element there,
    /// and the alternative is bundling a decoder. Callers fall back to handing
    /// the file to the system.
    pub struct Player;

    /// No decoder outside macOS, so nothing to draw before playback either.
    pub fn poster(_path: &Path) -> Option<Vec<u8>> {
        None
    }

    impl Player {
        pub fn open(_path: &Path) -> Option<Self> {
            None
        }
        pub fn play(&self) {}
        pub fn pause(&self) {}
        pub fn toggle(&self) {}
        pub fn is_playing(&self) -> bool {
            false
        }
        pub fn finished(&self) -> bool {
            false
        }
        pub fn size(&self) -> Option<(f32, f32)> {
            None
        }
        pub fn position(&self) -> Duration {
            Duration::ZERO
        }
        pub fn duration(&self) -> Option<Duration> {
            None
        }
        pub fn seek(&self, _fraction: f32) {}
    }
}

pub use platform::{Player, poster};

/// Whether a file is one this can even try to open. Cheap and by extension,
/// because opening it to find out costs a decoder.
pub fn is_video(path: &std::path::Path) -> bool {
    crate::data::attachment::content_type(path).starts_with("video/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn recognises_the_containers_signal_sends() {
        assert!(is_video(Path::new("/tmp/clip.mp4")));
        assert!(is_video(Path::new("/tmp/clip.mov")));
        assert!(is_video(Path::new("/tmp/clip.webm")));
    }

    #[test]
    fn does_not_claim_everything() {
        assert!(!is_video(Path::new("/tmp/photo.jpg")));
        assert!(!is_video(Path::new("/tmp/note.m4a")));
        assert!(!is_video(Path::new("/tmp/mystery")));
    }
}
