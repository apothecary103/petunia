//! Images at the resolution the display actually has.
//!
//! gpui uploads an image to its atlas at the image's own pixel size and lets the
//! GPU sample it with a bilinear filter and no mipmaps. Minifying a 640px avatar
//! into a 34pt circle is then a single tap per output pixel, which throws away
//! nineteen twentieths of the source and aliases hard — the "pixelated on a
//! retina display" symptom. Resampling on the CPU with a real filter, to exactly
//! the number of device pixels the element will occupy, is the fix.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    App, Asset, ImageCacheError, Img, ObjectFit, RenderImage, Window, img, px,
};
use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, Frame, ImageFormat, Rgba, RgbaImage};

/// A decoded, resampled image, keyed by the file and the size it is drawn at.
pub enum Scaled {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Request {
    path: PathBuf,
    /// Device pixels, so the same file at two sizes is two entries and neither
    /// is resampled twice.
    width: u32,
    height: u32,
}

impl Asset for Scaled {
    type Source = Request;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        source: Self::Source,
        _cx: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        // Runs on gpui's background executor, so decoding here blocks nothing.
        async move { decode(&source) }
    }
}

/// An animation longer than this is almost certainly not worth the memory: a
/// 512-square frame is a megabyte of RGBA, and a sticker pack full of them will
/// exhaust the atlas before it entertains anyone.
const MAX_FRAMES: usize = 120;

fn decode(request: &Request) -> Result<Arc<RenderImage>, ImageCacheError> {
    let bytes = std::fs::read(&request.path)?;
    let format = image::guess_format(&bytes)?;

    let frames = match format {
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

/// Scales to fit the requested box at the source's own aspect ratio, and never
/// scales up — enlarging a small image on the CPU only wastes memory, since the
/// GPU's bilinear filter does the same job for free.
fn resample(source: RgbaImage, request: &Request) -> RgbaImage {
    let (width, height) = source.dimensions();
    let scale = (request.width as f32 / width as f32)
        .min(request.height as f32 / height as f32)
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

/// A picture drawn at exactly this size, resampled for the display it lands on.
/// Both axes are given because an image's natural size is its pixel size, and
/// letting the layout infer one from the other is how a screenshot ends up
/// thousands of units wide.
pub fn picture(path: impl AsRef<Path>, width: f32, height: f32) -> Img {
    sized(path.as_ref().to_path_buf(), width, height)
        .w(px(width))
        .h(px(height))
        .object_fit(ObjectFit::Contain)
}

/// A picture filling a square and cropped to it, for avatars and thumbnails.
pub fn cropped(path: impl AsRef<Path>, edge: f32) -> Img {
    sized(path.as_ref().to_path_buf(), edge, edge)
        .size(px(edge))
        .object_fit(ObjectFit::Cover)
}

/// The scale factor is only knowable inside a window, so the request is built
/// per frame rather than at construction. Requests are cheap; the resampled
/// result behind them is cached by the asset system.
fn sized(path: PathBuf, width: f32, height: f32) -> Img {
    img(move |window: &mut Window, cx: &mut App| {
        let scale = window.scale_factor().max(1.0);
        let device = |value: f32| ((value * scale).ceil() as u32).max(1);

        let request = Request {
            path: path.clone(),
            width: device(width),
            height: device(height),
        };
        window.use_asset::<Scaled>(&request, cx)
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

    #[test]
    fn reports_a_file_it_cannot_read() {
        assert!(decode(&request(Path::new("/nonexistent/image.png"), 10, 10)).is_err());
    }
}
