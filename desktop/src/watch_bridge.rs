use gpui::Context;
use tokio::sync::watch;

/// Spawns a task on gpui's own executor that awaits changes on `rx` and calls
/// `cx.notify()` on the entity owning `cx` whenever one arrives. Stops once that
/// entity is dropped.
///
/// `AsyncApp`/`WeakEntity` aren't `Send` in this gpui fork, so this runs on gpui's
/// foreground executor via `cx.spawn` rather than on the tokio runtime. That's fine:
/// `watch::Receiver::changed()` doesn't touch the tokio I/O driver, so it can be
/// polled from any executor.
pub fn notify_on_change<T, V>(mut rx: watch::Receiver<T>, cx: &mut Context<V>)
where
    T: 'static,
    V: 'static,
{
    cx.spawn(async move |this, cx| {
        while rx.changed().await.is_ok() {
            if let Err(e) = this.update(cx, |_, cx| cx.notify()) {
                tracing::error!("Failed to notify: {:?}", e);
                break;
            }
        }
    })
    .detach();
}
