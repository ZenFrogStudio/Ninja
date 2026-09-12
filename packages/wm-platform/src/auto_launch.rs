use crate::{platform_impl, Result};

/// Whether this executable is registered to start when the user logs in.
///
/// # Platform-specific
///
/// - **Windows**: True only if the `Run` registry entry names this exact
///   executable and hasn't been switched off in Task Manager's Startup
///   tab. An entry pointing at another copy of the binary reads as
///   disabled.
/// - **macOS**: Checks the launch agent for the app name.
pub fn is_run_on_startup_enabled() -> Result<bool> {
  platform_impl::is_run_on_startup_enabled()
}

/// Registers or unregisters this executable to start when the user logs
/// in.
///
/// # Platform-specific
///
/// - **Windows**: Writes the quoted executable path to the `Run` registry
///   key, overwriting any entry left by another copy of the binary.
/// - **macOS**: Adds or removes a launch agent.
pub fn set_run_on_startup(enabled: bool) -> Result<()> {
  platform_impl::set_run_on_startup(enabled)
}
