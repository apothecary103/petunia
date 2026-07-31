//! Opens a video the way the viewer does and reports whether frames arrive.
//!
//! Not a test: `AVPlayer` wants the main thread, and a test harness does not
//! give you one. `cargo run --example video_probe -- <file>`.

// The probe drives one corner of the module; the rest is the app's.
#[allow(dead_code, unused_imports)]
#[path = "../src/video.rs"]
mod video;

/// `video.rs` asks the data layer what a file is; pulling the real one in would
/// drag the whole Signal stack behind it, which has nothing to do with decoding.
/// The probe is handed a path and told to try, so the answer is always yes.
mod data {
    pub mod attachment {
        pub fn content_type(_path: &std::path::Path) -> String {
            "video/mp4".to_string()
        }
    }
}

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: video_probe <file>");
        return;
    };
    let path = std::path::PathBuf::from(path);
    println!("exists:   {}", path.exists());

    let Some(mut player) = video::Player::open(&path) else {
        println!("open:     FAILED");
        return;
    };
    println!("open:     ok");
    println!("duration: {:?}", player.duration());

    player.play();
    for tick in 0..40 {
        // AVFoundation loads the asset and fills the output on the main run
        // loop. Sleeping does not pump it; gpui's own loop does.
        unsafe {
            objc2_core_foundation::CFRunLoop::run_in_mode(
                objc2_core_foundation::kCFRunLoopDefaultMode,
                0.1,
                false,
            );
        }
        let frame = player.frame();
        println!(
            "{tick:>3}  playing={} pos={:?} duration={:?} frame={}",
            player.is_playing(),
            player.position(),
            player.duration(),
            frame.is_some(),
        );
        if tick > 8 && let Some(frame) = frame {
            
            let _ = &frame;
            println!(
                "frames are arriving, format {:?}",
                std::str::from_utf8(&frame.get_pixel_format().to_be_bytes())
            );
            return;
        }
    }
    println!("no frames arrived");
}
