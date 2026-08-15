// SPDX-License-Identifier: MIT AND Apache-2.0

//! Process-wide cleanup registry for signal termination.
//!
//! RAII guards restore process state (lockfiles, git refs) on `Drop`, but
//! destructors do not run when the process is terminated by a signal (e.g.
//! Ctrl+C). Guards register a cleanup callback here. A single signal handler
//! thread runs all registered callbacks in reverse registration order.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[cfg(not(windows))]
use signal_hook::consts::SIGHUP;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

/// Signals that trigger cleanup: SIGINT (Ctrl+C) and SIGTERM (polite kill), plus SIGHUP (terminal
/// hangup) on platforms that have it.
#[cfg(not(windows))]
const SIGNALS: &[i32] = &[SIGINT, SIGTERM, SIGHUP];
/// Signals that trigger cleanup: SIGINT (Ctrl+C) and SIGTERM (polite kill).
#[cfg(windows)]
const SIGNALS: &[i32] = &[SIGINT, SIGTERM];

/// A cleanup action run on signal termination.
type Cleanup = Box<dyn FnOnce() + Send>;

/// Process-wide stack of cleanup callbacks, (id, callback) pairs.
///
/// Global by necessity. The signal handler thread requires `'static` data and must be able to run
/// after the main thread is gone.
fn stack() -> &'static Mutex<Vec<(u64, Cleanup)>> {
    static STACK: OnceLock<Mutex<Vec<(u64, Cleanup)>>> = OnceLock::new();
    STACK.get_or_init(|| Mutex::new(Vec::new()))
}

/// Token for a registered cleanup callback.
///
/// Deregisters the callback on drop. Guards hold this as a field declared after their state
/// fields, so the callback is deregistered once the guard's own `Drop` has run the cleanup
/// normally and cannot run twice.
pub struct Registration {
    id: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut stack) = stack().lock() {
            stack.retain(|(id, _)| *id != self.id);
        }
    }
}

/// Register a cleanup callback to run on signal termination, best effort.
pub fn register(cleanup: impl FnOnce() + Send + 'static) -> Registration {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    install_signal_handler();
    if let Ok(mut stack) = stack().lock() {
        stack.push((id, Box::new(cleanup)));
    }

    Registration { id }
}

/// Install the signal handler thread (once per process, best effort).
fn install_signal_handler() {
    static HANDLER: OnceLock<()> = OnceLock::new();
    HANDLER.get_or_init(|| {
        if let Ok(mut signals) = Signals::new(SIGNALS) {
            std::thread::spawn(move || {
                // Blocks until the first signal arrives.
                if let Some(signal) = signals.forever().next() {
                    if let Ok(mut stack) = stack().lock() {
                        for (_, cleanup) in stack.drain(..).rev() {
                            // One failing cleanup must not skip the rest.
                            let _ = catch_unwind(AssertUnwindSafe(cleanup));
                        }
                    }
                    // Die as if we had never intercepted the signal. Fall back to the
                    // conventional 128 + signal exit code.
                    if signal_hook::low_level::emulate_default_handler(signal).is_err() {
                        std::process::exit(128 + signal);
                    }
                }
            });
        }
    });
}
