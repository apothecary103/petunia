use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{App, Entity};

use super::worker;
use crate::store::Store;

/// Starts the Signal worker and forwards everything it emits into the store.
pub fn spawn(store: Entity<Store>, cx: &mut App) {
    let (sender, mut receiver) = mpsc::channel(64);

    // presage's receive futures are huge (multi-MB in debug builds); the
    // default 2 MiB thread stack overflows while polling them.
    std::thread::Builder::new()
        .name("signal".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || worker::run(sender))
        .expect("spawn signal worker thread");

    cx.spawn(async move |cx| {
        while let Some(event) = receiver.next().await {
            // Whatever else is already waiting goes in with it. Every `apply`
            // notifies, and every view observes the store, so one event is one
            // full re-render of the window -- and startup emits a profile, an
            // avatar and a preview for every contact you have, back to back.
            let mut batch = vec![event];
            while batch.len() < BATCH {
                // An error here is either "nothing waiting" or "the worker hung
                // up"; both mean this batch is complete, and the outer loop
                // notices the second one on its next await.
                match receiver.try_recv() {
                    Ok(event) => batch.push(event),
                    Err(_) => break,
                }
            }

            store.update(cx, |store, cx| {
                for event in batch {
                    store.apply(event, cx);
                }
            });
        }
    })
    .detach();
}

/// How many events one update may carry. Large enough that a burst collapses
/// into a repaint or two, small enough that a busy stream cannot starve the
/// window of frames.
const BATCH: usize = 256;
