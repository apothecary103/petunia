//! The blur behind the conversation list.
//!
//! gpui's own `WindowBackgroundAppearance::Blurred` puts an `NSVisualEffectView`
//! across the whole window using the `Selection` material, which is a tint
//! rather than a blur — through it the desktop is simply *there*, in focus. What
//! a sidebar wants is the `Sidebar` material, which is what Finder and Mail use
//! and what people mean by a blurred sidebar.
//!
//! So the window is merely transparent, and this puts one view of its own behind
//! the content, sized to the list. Per-region, which gpui has no way to express,
//! and thick, which it has no way to ask for.
//!
//! No `unsafe` here: everything it touches is safe in objc2's bindings once the
//! main thread has been established, which `MainThreadMarker` does.

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::RefCell;

    use objc2::MainThreadOnly;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplication, NSAutoresizingMaskOptions, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

    thread_local! {
        /// The view, kept rather than looked up. `NSView`'s tag is read-only
        /// without subclassing, and this is main-thread-only anyway -- which is
        /// the same thread every call arrives on, since it is a render.
        static VIEW: RefCell<Option<Retained<NSVisualEffectView>>> = const { RefCell::new(None) };
    }

    /// Puts the blur behind the leftmost `width` points of the window, or takes
    /// it away when the list is not showing.
    ///
    /// Called per frame and cheap when nothing has changed: the view is held
    /// rather than looked up, and its frame is only written when it differs.
    pub fn sidebar(width: f32, showing: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };

        VIEW.with(|held| {
            let mut held = held.borrow_mut();

            if !showing {
                if let Some(view) = held.take() {
                    view.removeFromSuperview();
                }
                return;
            }

            // The key window, falling back to the only one there is: petunia
            // opens exactly one, and picking by index would quietly become wrong
            // the day that changes.
            let app = NSApplication::sharedApplication(mtm);
            let Some(window) = app.keyWindow().or_else(|| app.windows().iter().next()) else {
                return;
            };
            let Some(content) = window.contentView() else {
                return;
            };

            let wanted = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(f64::from(width), content.bounds().size.height),
            );

            if let Some(view) = held.as_ref() {
                if view.frame() != wanted {
                    view.setFrame(wanted);
                }
                return;
            }

            let view = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), wanted);
            // The material a sidebar is supposed to use. `Selection`, which gpui
            // picks, is a tint and reads as plain transparency.
            view.setMaterial(NSVisualEffectMaterial::Sidebar);
            // Behind the window: what is being blurred is the desktop, not
            // whatever petunia drew underneath.
            view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            // Active regardless of focus, or the blur flattens to a grey slab
            // whenever the window is not frontmost.
            view.setState(NSVisualEffectState::Active);
            view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);

            content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
            *held = Some(view);
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    /// Nothing to do: vibrancy is a macOS idea, and there is no equivalent to
    /// put behind a window elsewhere that is worth pretending about.
    pub fn sidebar(_width: f32, _showing: bool) {}
}

pub use platform::sidebar;
