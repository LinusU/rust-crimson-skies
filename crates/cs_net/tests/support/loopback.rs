//! The process-wide loopback-socket lock every socket test holds.
//!
//! Measured on an 11-core machine deliberately oversubscribed more than
//! twofold: with many short-lived socket pairs churning at once, 380 of 1500
//! loopback pairs lost one or two of 59 datagrams each and eleven loopback
//! handshakes at once left 435 of 1500 unable to finish inside the connection
//! layer's own window, while a single pair at the same load settled 600 of 600
//! (`docs/findings/2026-10-04-f54-x2-loopback-pump-and-socket-determinism.md`,
//! sections "Concurrency" and 4-6). The loss is in the machine's loopback UDP
//! path, not in anything a test asserts, so this binary keeps at most one live
//! loopback workload at a time instead of racing for the loopback path. Each
//! locked test is a few milliseconds of socket work, so serializing costs
//! nothing measurable and takes the machine's load out of the result.
//!
//! When every test file was its own binary the lock had to be per file —
//! separate processes cannot share a mutex. As modules of this one binary,
//! `accept_f54_b_pinned_transport`, `accept_f54_c_lifecycle` and
//! `accept_f54_x10_loopback_loss` hold the same lock, so the serialization the
//! measurements motivated now covers every loopback test in the crate.

use std::sync::{Mutex, MutexGuard};

static LOOPBACK: Mutex<()> = Mutex::new(());

/// Takes the loopback socket lock. Held for as long as the sockets it guards.
pub fn lock() -> MutexGuard<'static, ()> {
    LOOPBACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
