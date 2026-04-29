//! Panic hook that restores the terminal before the default handler prints
//! the panic message (per ADR-006 + US-01 AC-5).
//!
//! Without this, a panic during ratatui rendering leaves the terminal in
//! raw mode + alternate screen, garbling subsequent shell output.

use std::io;
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

static INSTALL_ONCE: Once = Once::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install the panic hook. Idempotent — safe to call from `main()` and from
/// test harnesses. The hook calls `restore_terminal()` then chains to the
/// default Rust panic handler so backtraces and `RUST_BACKTRACE` continue to
/// work as expected.
pub fn install_panic_hook() {
    INSTALL_ONCE.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            // Best-effort terminal restoration. Swallow errors — we are about
            // to crash anyway; the user must see the panic message regardless.
            let _ = restore_terminal();
            default_hook(info);
        }));
        INSTALLED.store(true, Ordering::SeqCst);
    });
}

/// Test-only probe: returns true once `install_panic_hook` has been called.
/// Used by the unit-test inner loop to verify idempotency.
pub fn is_installed_for_tests() -> bool {
    INSTALLED.load(Ordering::SeqCst)
}

/// Disable raw mode and leave the alternate screen, restoring the user's
/// shell to a usable state. Public so the composition root can call it on
/// graceful exit too.
pub fn restore_terminal() -> io::Result<()> {
    use crossterm::{cursor, execute, terminal};
    if terminal::is_raw_mode_enabled().unwrap_or(false) {
        terminal::disable_raw_mode()?;
    }
    let mut stdout = io::stdout();
    execute!(
        stdout,
        terminal::LeaveAlternateScreen,
        cursor::Show,
        crossterm::style::ResetColor,
    )?;
    Ok(())
}
