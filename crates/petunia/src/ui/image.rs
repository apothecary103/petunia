//! Images at the resolution the display actually has.
//!
//! gpui uploads an image to its atlas at the image's own pixel size and lets the
//! GPU sample it with a bilinear filter and no mipmaps. Minifying a 640px avatar
//! into a 34pt circle is then a single tap per output pixel, which throws away
//! nineteen twentieths of the source and aliases hard — the "pixelated on a
//! retina display" symptom. Resampling on the CPU with a real filter, to exactly
//! the number of device pixels the element will occupy, is the fix.
//!
//! And the resamples are kept here rather than in gpui's asset cache, which
//! never gives anything back: see `Cache`.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::FutureExt;
use futures::future::Shared;
use gpui::prelude::*;
use gpui::{
    App, Global, ImageCacheError, Img, ObjectFit, RenderImage, Task, Window, img, px,
};
use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, Frame, ImageFormat, Rgba, RgbaImage};

/// What identifies a resample: the file, and the size and shape it is drawn at.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Request {
    path: PathBuf,
    /// Device pixels, so the same file at two sizes is two entries and neither
    /// is resampled twice.
    width: u32,
    height: u32,
    fit: Fit,
    kind: Kind,
}

/// Which picture, and how much of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The file, first frame only. Nothing without an element id can show a
    /// second one, so decoding the rest is a hundred resamples nobody will ever
    /// see -- which is what a grid of animated stickers spent.
    Still,
    /// The file, every frame of it.
    Animated,
    /// The picture *inside* the file: an album cover in an audio file's tags.
    /// Part of the key rather than a second cache, so a track's artwork is
    /// resampled and evicted like everything else -- and the alternative was
    /// writing covers out as files of their own into a cache directory whose
    /// layout means something.
    Cover,
}

/// Which way the box is honoured. `Contain` is the whole picture inside it;
/// `Cover` fills it and lets the element crop what hangs over.
///
/// The distinction is not cosmetic: resampling to *fit* and then drawing with
/// `ObjectFit::Cover` hands the GPU an image smaller than the box in one axis and
/// asks it to enlarge it, which is how a wide photograph became a blurry square
/// thumbnail. The crop has to be decided before the resample, not after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fit {
    Contain,
    Cover,
}

type Decoded = Result<Arc<RenderImage>, ImageCacheError>;

/// The resamples that have been drawn, and the last frame each was drawn on.
///
/// This exists because `Window::use_asset` does not: gpui keeps a loaded asset in
/// `App::loading_assets` for the life of the process and offers no eviction of
/// its own, and a resample is uncompressed BGRA — a photograph in the
/// conversation is a megabyte or two per size it was drawn at, and one animation
/// is up to `MAX_FRAMES` of them. Scrolling back through a thread full of
/// pictures therefore never gave a byte back, which is the whole of "petunia's
/// memory use is high". So the resamples are held here instead, and the ones that
/// have not been drawn in a while are handed back — both the buffers and the
/// atlas tile, which is a second copy on the GPU.
struct Cache {
    entries: HashMap<Request, Entry>,
    /// Not a clock: a counter bumped per lookup, which is all "least recently
    /// drawn" needs and is the one thing available inside a render.
    drawn: u64,
}

struct Entry {
    decoding: Shared<Task<Decoded>>,
    drawn: u64,
    /// What it costs, once it has finished decoding. Zero while it is still being
    /// made, which is also the only honest answer then.
    bytes: usize,
}

impl Global for Cache {}

/// What the resamples may take between them.
///
/// A budget in bytes rather than in entries, because the two have no fixed
/// relation: a photograph in a conversation is a megabyte or two and a sticker
/// tile is twelve kilobytes, so any single count is either far too small for a
/// grid of the second or far too generous for a page of the first. Counted, a
/// sticker picker asked for a thousand entries against a ceiling of a couple of
/// hundred and evicted everything else in the window to get them -- including
/// every avatar on screen, which was then asked for again on the next frame and
/// evicted again on the one after. That is the whole of "opening the stickers
/// makes every picture in the application disappear".
///
/// Comfortably more than any one frame can ask for, which is the invariant that
/// matters: evicting something being drawn now is asking to decode it again next
/// frame, forever.
const BUDGET: usize = 256 * 1024 * 1024;

impl Cache {
    /// The resample for this request, or `None` while it is still being made.
    fn load(request: &Request, window: &mut Window, cx: &mut App) -> Option<Decoded> {
        if !cx.has_global::<Cache>() {
            cx.set_global(Cache {
                entries: HashMap::new(),
                drawn: 0,
            });
        }

        let known = cx.update_global(|cache: &mut Cache, _| {
            cache.drawn += 1;
            let drawn = cache.drawn;
            let entry = cache.entries.get_mut(request)?;
            entry.drawn = drawn;
            Some(entry.decoding.clone())
        });

        // A task that has not finished yet reports nothing rather than blocking
        // the frame on it.
        if let Some(decoding) = known {
            return decoding.now_or_never();
        }

        let decoding = {
            let source = request.clone();
            // On the background executor, so decoding a photograph blocks no
            // frame.
            cx.background_executor()
                .spawn(async move { decode(&source) })
                .shared()
        };
        cx.update_global(|cache: &mut Cache, _| {
            let drawn = cache.drawn;
            cache.entries.insert(request.clone(), Entry {
                decoding: decoding.clone(),
                drawn,
                bytes: 0,
            });
        });
        Self::evict(window, cx);

        // Redrawn when it lands, since the frame that asked has nothing to show.
        let view = window.current_view();
        window
            .spawn(cx, {
                let decoding = decoding.clone();
                async move |cx| {
                    let _ = decoding.await;
                    cx.on_next_frame(move |_, cx| cx.notify(view));
                }
            })
            .detach();

        decoding.now_or_never()
    }

    /// Gives back the least recently drawn resamples until the rest fit in
    /// `BUDGET`.
    fn evict(window: &mut Window, cx: &mut App) {
        let stale = cx.update_global(|cache: &mut Cache, _| {
            let mut total = 0;
            let mut by_age = Vec::with_capacity(cache.entries.len());
            for (request, entry) in cache.entries.iter_mut() {
                // What a decode cost is only knowable once it has finished, and
                // a task is polled here rather than awaited: an entry still
                // being made weighs nothing yet, which it does not.
                if entry.bytes == 0
                    && let Some(Ok(image)) = entry.decoding.clone().now_or_never()
                {
                    entry.bytes = weight(&image);
                }
                total += entry.bytes;
                by_age.push((entry.drawn, request.clone()));
            }
            if total <= BUDGET {
                return Vec::new();
            }
            by_age.sort_unstable_by_key(|(drawn, _)| *drawn);

            let mut freed = Vec::new();
            for (_, request) in by_age {
                if total <= BUDGET {
                    break;
                }
                if let Some(entry) = cache.entries.remove(&request) {
                    total = total.saturating_sub(entry.bytes);
                    freed.extend(entry.decoding.now_or_never());
                }
            }
            freed
        });

        // The buffers go with the task above; this is the copy in the atlas,
        // which nothing else would ever reclaim.
        for image in stale.into_iter().flatten() {
            cx.drop_image(image, Some(window));
        }
    }
}

/// What a resample takes, every frame of it. Uncompressed BGRA, so this is the
/// pixels and nothing else.
fn weight(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .filter_map(|frame| image.as_bytes(frame))
        .map(<[u8]>::len)
        .sum()
}

/// An animation longer than this is almost certainly not worth the memory: a
/// 512-square frame is a megabyte of RGBA, and a sticker pack full of them will
/// exhaust the atlas before it entertains anyone.
const MAX_FRAMES: usize = 120;

fn decode(request: &Request) -> Result<Arc<RenderImage>, ImageCacheError> {
    let bytes = match request.kind {
        Kind::Cover => petunia_media::song::cover(&request.path).ok_or_else(|| {
            ImageCacheError::Asset(
                format!("{} has no cover art", request.path.display()).into(),
            )
        })?,
        _ => std::fs::read(&request.path)?,
    };
    let format = image::guess_format(&bytes)?;

    let frames = match format {
        _ if request.kind != Kind::Animated => {
            vec![still(image::load_from_memory_with_format(&bytes, format)?, request)]
        }
        ImageFormat::Gif => resample_all(GifDecoder::new(Cursor::new(&bytes))?.into_frames(), request),
        ImageFormat::WebP => {
            let mut decoder = WebPDecoder::new(Cursor::new(&bytes))?;
            if decoder.has_animation() {
                let _ = decoder.set_background_color(Rgba([0, 0, 0, 0]));
                resample_all(decoder.into_frames(), request)
            } else {
                vec![still(image::load_from_memory_with_format(&bytes, format)?, request)]
            }
        }
        _ => vec![still(image::load_from_memory_with_format(&bytes, format)?, request)],
    };

    if frames.is_empty() {
        return Err(ImageCacheError::Asset(
            format!("no frame of {} could be decoded", request.path.display()).into(),
        ));
    }
    Ok(Arc::new(RenderImage::new(frames)))
}

fn still(image: image::DynamicImage, request: &Request) -> Frame {
    Frame::new(resample(image.into_rgba8(), request))
}

fn resample_all(frames: image::Frames<'_>, request: &Request) -> Vec<Frame> {
    frames
        .take(MAX_FRAMES)
        .filter_map(Result::ok)
        .map(|frame| {
            let delay = frame.delay();
            Frame::from_parts(resample(frame.into_buffer(), request), 0, 0, delay)
        })
        .collect()
}

/// Scales to the requested box at the source's own aspect ratio, and never
/// scales up — enlarging a small image on the CPU only wastes memory, since the
/// GPU's bilinear filter does the same job for free.
fn resample(source: RgbaImage, request: &Request) -> RgbaImage {
    let (width, height) = source.dimensions();
    let across = request.width as f32 / width as f32;
    let down = request.height as f32 / height as f32;
    let scale = match request.fit {
        Fit::Contain => across.min(down),
        // The larger ratio, so the shorter axis reaches the box and the longer
        // one overhangs to be cropped.
        Fit::Cover => across.max(down),
    }
    .min(1.0);

    let mut resized = if scale < 1.0 {
        let target = |value: u32| ((value as f32 * scale).round() as u32).max(1);
        image::imageops::resize(&source, target(width), target(height), FilterType::Lanczos3)
    } else {
        source
    };

    // gpui's atlas is BGRA; `image` decodes RGBA.
    for pixel in resized.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    resized
}

/// The shape the file on disk actually is.
///
/// What a message is drawn at cannot come from the sender's declaration. It is
/// often absent -- and always absent for a picture of our own, until the thread
/// is reloaded and the cache measures it -- in which case the box falls back to
/// the whole of `image_max_*`, a 4:3 rectangle nothing but a 4:3 picture fills.
/// It can also simply disagree with the bytes: a photograph off a phone is stored
/// landscape with an EXIF rotation, and Signal declares the *rotated* dimensions,
/// so a portrait box gets drawn around a landscape picture. Either way the
/// difference appears as margin inside the element, because a contained picture
/// centres itself in whatever box it was given.
///
/// A header read is a few hundred bytes however large the picture is, but a
/// visible row is rebuilt every frame, so the answer is kept -- including the
/// failure, so a file that is not an image is not re-read forever.
pub fn shape(path: &Path) -> Option<petunia_data::attachment::Size> {
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Enough for what is on screen and the overdraw around it, several times
    /// over.
    const CAPACITY: usize = 256;

    thread_local! {
        static SHAPES: RefCell<HashMap<PathBuf, Option<petunia_data::attachment::Size>>> =
            RefCell::new(HashMap::new());
    }

    SHAPES.with(|shapes| {
        let mut shapes = shapes.borrow_mut();
        if let Some(shape) = shapes.get(path) {
            return *shape;
        }

        let shape = petunia_data::attachment::dimensions(path);
        if shapes.len() >= CAPACITY {
            shapes.clear();
        }
        shapes.insert(path.to_path_buf(), shape);
        shape
    })
}

/// A picture drawn at exactly this size, resampled for the display it lands on.
/// Both axes are given because an image's natural size is its pixel size, and
/// letting the layout infer one from the other is how a screenshot ends up
/// thousands of units wide.
pub fn picture(path: impl AsRef<Path>, width: f32, height: f32) -> Img {
    contained(path.as_ref().to_path_buf(), width, height, Kind::Still)
}

fn contained(path: PathBuf, width: f32, height: f32, kind: Kind) -> Img {
    sized(path, width, height, Fit::Contain, kind)
        .w(px(width))
        .h(px(height))
        .object_fit(ObjectFit::Contain)
}

/// The cover art inside an audio file, filling a square the way an album cover
/// is always reproduced.
pub fn artwork(path: impl AsRef<Path>, edge: f32) -> Img {
    sized(path.as_ref().to_path_buf(), edge, edge, Fit::Cover, Kind::Cover)
        .size(px(edge))
        .object_fit(ObjectFit::Cover)
}

/// The same, for a file that might be animated. Which is also the only call that
/// decodes past the first frame: `picture` cannot show a second one, so a GIF
/// drawn through it used to cost a hundred resamples to display one of them.
///
/// An `Img` keeps which frame it is showing in gpui's element state, and element
/// state is keyed by the element's id — so an image with no id is handed `None`
/// for its state every frame, leaves the counter at zero, and draws frame one of
/// the animation forever. That was every GIF in the application: decoded whole,
/// resampled whole, and frozen. Nothing else needs an id, and an id has to be
/// unique among its siblings, so this is a separate call rather than something
/// `picture` invents for every avatar in a list.
pub fn animated(
    id: impl Into<gpui::ElementId>,
    path: impl AsRef<Path>,
    width: f32,
    height: f32,
) -> gpui::Stateful<Img> {
    contained(path.as_ref().to_path_buf(), width, height, Kind::Animated).id(id.into())
}

/// A picture filling a square and cropped to it, for avatars and thumbnails.
pub fn cropped(path: impl AsRef<Path>, edge: f32) -> Img {
    sized(path.as_ref().to_path_buf(), edge, edge, Fit::Cover, Kind::Still)
        .size(px(edge))
        .object_fit(ObjectFit::Cover)
}

/// The scale factor is only knowable inside a window, so the request is built
/// per frame rather than at construction. Requests are cheap; the resampled
/// result behind them is cached by the asset system.
fn sized(path: PathBuf, width: f32, height: f32, fit: Fit, kind: Kind) -> Img {
    img(move |window: &mut Window, cx: &mut App| {
        let scale = window.scale_factor().max(1.0);
        let device = |value: f32| ((value * scale).ceil() as u32).max(1);

        let request = Request {
            path: path.clone(),
            width: device(width),
            height: device(height),
            fit,
            kind,
        };
        Cache::load(&request, window, cx)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn request(path: &Path, width: u32, height: u32) -> Request {
        Request {
            path: path.to_path_buf(),
            width,
            height,
            fit: Fit::Contain,
            kind: Kind::Still,
        }
    }

    fn covering(path: &Path, edge: u32) -> Request {
        Request {
            fit: Fit::Cover,
            ..request(path, edge, edge)
        }
    }

    fn write(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    /// The whole point: a large source is reduced to the pixels the display has,
    /// not handed to the GPU at full size to be minified in one bilinear tap.
    #[test]
    fn resamples_down_to_the_requested_device_size() {
        let (_dir, path) = write(&png(640, 640));

        let decoded = decode(&request(&path, 68, 68)).unwrap();

        assert_eq!(decoded.size(0).width.0, 68);
        assert_eq!(decoded.size(0).height.0, 68);
    }

    #[test]
    fn keeps_the_source_aspect_ratio() {
        let (_dir, path) = write(&png(800, 400));

        let decoded = decode(&request(&path, 400, 400)).unwrap();

        assert_eq!(decoded.size(0).width.0, 400);
        assert_eq!(decoded.size(0).height.0, 200);
    }

    /// A thumbnail is cropped, so the box has to be *filled* before the element
    /// crops it. Resampled to fit instead, a wide photograph arrives shorter than
    /// the square it is drawn in and the GPU enlarges it to cover — which is what
    /// made the shared-media grid blurry.
    #[test]
    fn a_cropped_thumbnail_fills_the_box() {
        let (_dir, path) = write(&png(800, 400));

        let decoded = decode(&covering(&path, 100)).unwrap();

        assert_eq!(decoded.size(0).width.0, 200);
        assert_eq!(decoded.size(0).height.0, 100);
    }

    /// Enlarging on the CPU costs memory and buys nothing the GPU's own filter
    /// does not already do.
    #[test]
    fn never_scales_up() {
        let (_dir, path) = write(&png(32, 32));

        let decoded = decode(&request(&path, 400, 400)).unwrap();

        assert_eq!(decoded.size(0).width.0, 32);
    }

    /// The atlas is BGRA and `image` decodes RGBA, so a channel swap that goes
    /// missing turns every red pixel blue.
    #[test]
    fn writes_bgra_not_rgba() {
        let (_dir, path) = write(&png(4, 4));

        let decoded = decode(&request(&path, 4, 4)).unwrap();
        let pixel = &decoded.as_bytes(0).unwrap()[..4];

        assert_eq!(pixel, [30, 20, 10, 255]);
    }

    /// An element with no id cannot show a second frame, so decoding one is a
    /// megabyte a piece for nothing -- which is what a grid of animated stickers
    /// spent, and what emptied the cache of every other picture in the window.
    #[test]
    fn a_still_request_decodes_one_frame_of_an_animation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("animation.gif");
        std::fs::write(&path, gif(3)).unwrap();

        assert_eq!(decode(&request(&path, 8, 8)).unwrap().frame_count(), 1);
        assert_eq!(
            decode(&Request {
                kind: Kind::Animated,
                ..request(&path, 8, 8)
            })
            .unwrap()
            .frame_count(),
            3
        );
    }

    fn gif(frames: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(Cursor::new(&mut bytes));
            encoder.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
            for _ in 0..frames {
                encoder
                    .encode_frame(Frame::new(RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255]))))
                    .unwrap();
            }
        }
        bytes
    }

    #[test]
    fn reports_a_file_it_cannot_read() {
        assert!(decode(&request(Path::new("/nonexistent/image.png"), 10, 10)).is_err());
    }
}
