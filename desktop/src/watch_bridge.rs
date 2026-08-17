use gpui::{Context, Window};
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

pub fn execute_on_change<T, V, F>(
    mut rx: watch::Receiver<T>,
    cx: &mut Context<V>,
    name: &'static str,
    f: F,
) where
    T: Clone + 'static,
    V: 'static,
    F: Fn(&mut V, &mut Window, &mut Context<V>, T, T) + 'static,
{
    let mut prev = rx.borrow().clone();

    cx.spawn(async move |this, cx| {
        while rx.changed().await.is_ok() {
            if this.upgrade().is_none() {
                tracing::debug!("Stopping listener, {} entity is gone", name);
                break;
            }

            let val = rx.borrow().clone();

            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                this.update_in(cx, |v, window, cx| {
                    f(v, window, cx, val.clone(), prev.clone())
                })
            })) {
                Ok(Ok(())) => {
                    prev = val;
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        "Skipping update for {} listener, window unavailable: {:?}",
                        name,
                        e
                    );
                }
                Err(e) => {
                    tracing::error!("Panic while executing on change for {}: {:?}", name, e);
                    prev = val;
                }
            }
        }
        tracing::debug!("Listener for {} stopped", name);
    })
    .detach();
}
