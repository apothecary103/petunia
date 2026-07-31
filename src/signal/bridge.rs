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
            store.update(cx, |store, cx| store.apply(event, cx));
        }
    })
    .detach();
}
