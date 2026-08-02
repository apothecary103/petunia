//! Which spoilers have been uncovered.
//!
//! A spoiler is a rounded block painted *over* the words rather than a
//! substitution for them: the text is laid out as it was written, so uncovering
//! one is a repaint rather than a reflow, and every offset a selection or a
//! highlight holds stays where it was. What a cover is asked about is whether it
//! is still a cover, and that question is answered in `ui::wash`'s paint — the
//! element that draws the block and the element that hears the click are the same
//! element, rebuilt every frame, with nowhere of its own to keep anything. So the
//! answer lives in a global, exactly as the one selection does.
//!
//! Keyed by the run the words are in and where in it the spoiler starts, which is
//! stable frame to frame and unique between two spoilers in one message.

use std::collections::HashSet;

use gpui::{App, Global, SharedString};

#[derive(Default)]
struct Uncovered(HashSet<(SharedString, usize)>);

impl Global for Uncovered {}

/// Whether this spoiler is still covered. Everything starts covered: that is
/// what the sender asked for.
pub fn covered(run: &SharedString, at: usize, cx: &App) -> bool {
    !cx.try_global::<Uncovered>()
        .is_some_and(|uncovered| uncovered.0.contains(&(run.clone(), at)))
}

/// Uncovers one, for as long as the application is running. Nothing covers it
/// again: a reader who has seen it has seen it, and a block that came back would
/// be a click to be made twice.
pub fn uncover(run: SharedString, at: usize, cx: &mut App) {
    cx.default_global::<Uncovered>().0.insert((run, at));
}
