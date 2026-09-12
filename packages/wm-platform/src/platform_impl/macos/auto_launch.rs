use auto_launch::AutoLaunch;

use crate::{Error, Result};

/// Whether this executable is registered to run at login.
pub(crate) fn is_run_on_startup_enabled() -> Result<bool> {
  instance()?
    .is_enabled()
    .map_err(|err| Error::Platform(err.to_string()))
}

/// Registers or unregisters this executable to run at login.
pub(crate) fn set_run_on_startup(enabled: bool) -> Result<()> {
  let instance = instance()?;

  let res = if enabled {
    instance.enable()
  } else {
    instance.disable()
  };

  res.map_err(|err| Error::Platform(err.to_string()))
}

/// Builds the launch agent entry for this executable.
fn instance() -> Result<AutoLaunch> {
  let exe_path = std::env::current_exe()?.to_string_lossy().to_string();
  let args: [&str; 0] = [];

  Ok(AutoLaunch::new("Ninja", &exe_path, false, &args))
}
