//! Hooks for servicing host-platform work during synchronous engine operations.
//!
//! Some platform entry points own queues that must be drained on the entry
//! thread even while a long game load is in progress. The hook is thread-local
//! for that reason: a background mission parse must never try to service the
//! Android main thread's looper.

use std::cell::Cell;

thread_local! {
    static EVENT_PUMP: Cell<Option<fn()>> = const { Cell::new(None) };
}

/// Set or clear the event pump for the current thread.
///
/// The callback must be non-blocking because load loops invoke it at regular
/// milestones. Runtimes without a platform queue leave the hook unset.
pub fn set_event_pump(pump: Option<fn()>) {
    EVENT_PUMP.set(pump);
}

/// Service pending platform events on the current thread, if configured.
#[inline]
pub fn service_events() {
    if let Some(pump) = EVENT_PUMP.get() {
        pump();
    }
}
