use std::io;

use winreg::{
  enums::{RegType::REG_BINARY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE},
  RegKey, RegValue,
};

use crate::Result;

/// Key whose values are launched by Explorer at logon.
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Key where Task Manager's Startup tab records entries it has enabled or
/// disabled. Its record overrides the `Run` value.
const STARTUP_APPROVED_KEY: &str =
  r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Name of the value under both keys.
const VALUE_NAME: &str = "Ninja";

/// `StartupApproved` payload for an enabled entry.
///
/// The format is undocumented: the first byte is the state (even for
/// enabled, odd for disabled), the last eight are a `FILETIME` of when it
/// was disabled.
const STARTUP_APPROVED_ENABLED: [u8; 12] =
  [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// Whether this executable is registered to run at logon.
///
/// Only an entry naming *this* executable counts. An entry left behind by
/// another copy of the binary — a dev build, or an install that has since
/// moved — reads as disabled, so enabling from here overwrites it instead
/// of being skipped as already done. An entry the user has switched off in
/// Task Manager's Startup tab reads as disabled too.
pub(crate) fn is_run_on_startup_enabled() -> Result<bool> {
  let hkcu = RegKey::predef(HKEY_CURRENT_USER);

  let stored_command = hkcu
    .open_subkey_with_flags(RUN_KEY, KEY_READ)
    .and_then(|key| key.get_value::<String, _>(VALUE_NAME))
    .ok();

  let approved_state = hkcu
    .open_subkey_with_flags(STARTUP_APPROVED_KEY, KEY_READ)
    .and_then(|key| key.get_raw_value(VALUE_NAME))
    .ok()
    .map(|value| value.bytes);

  Ok(is_enabled_for(
    stored_command.as_deref(),
    &startup_command()?,
    approved_state.as_deref(),
  ))
}

/// Registers or unregisters this executable to run at logon.
pub(crate) fn set_run_on_startup(enabled: bool) -> Result<()> {
  let hkcu = RegKey::predef(HKEY_CURRENT_USER);
  let (run_key, _) = hkcu.create_subkey(RUN_KEY)?;

  if !enabled {
    // A value that's already gone is the state being asked for.
    return match run_key.delete_value(VALUE_NAME) {
      Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
      result => Ok(result?),
    };
  }

  run_key.set_value(VALUE_NAME, &startup_command()?)?;

  // Clear a Task Manager "disabled" record, which would otherwise keep
  // overriding the value just written. The key only exists once the user
  // has toggled something in the Startup tab, so a missing key is fine.
  if let Ok(approved_key) =
    hkcu.open_subkey_with_flags(STARTUP_APPROVED_KEY, KEY_SET_VALUE)
  {
    approved_key.set_raw_value(
      VALUE_NAME,
      &RegValue {
        vtype: REG_BINARY,
        bytes: STARTUP_APPROVED_ENABLED.to_vec(),
      },
    )?;
  }

  Ok(())
}

/// Decides the enabled state from what the registry holds.
///
/// `stored_command` is the `Run` value, `expected_command` is what this
/// executable would write there, and `approved_state` is Task Manager's
/// record, if it has one. No record means the entry has never been
/// toggled there, which counts as enabled.
fn is_enabled_for(
  stored_command: Option<&str>,
  expected_command: &str,
  approved_state: Option<&[u8]>,
) -> bool {
  let is_this_executable = stored_command
    .is_some_and(|stored| stored.eq_ignore_ascii_case(expected_command));

  let is_approved = approved_state
    .is_none_or(|state| state.first().is_some_and(|byte| byte % 2 == 0));

  is_this_executable && is_approved
}

/// Command line stored in the `Run` key for this executable.
fn startup_command() -> Result<String> {
  let exe_path = std::env::current_exe()?;
  Ok(quote_path(&exe_path.to_string_lossy()))
}

/// Wraps a path in double quotes.
///
/// Explorer hands the stored string to `CreateProcess` as-is. Unquoted,
/// a path with spaces (`C:\Program Files\...`) is only found by trying
/// each prefix in turn, and stops working the moment a `C:\Program.exe`
/// appears.
fn quote_path(path: &str) -> String {
  format!("\"{path}\"")
}

#[cfg(test)]
mod tests {
  use super::{is_enabled_for, quote_path};

  const INSTALLED: &str = r#""C:\Program Files\Ninja\ninja.exe""#;
  const DEV_BUILD: &str = r#""D:\Ninja\target\release\ninja.exe""#;

  const APPROVED: &[u8] = &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  const DISAPPROVED: &[u8] =
    &[3, 0, 0, 0, 0x2b, 0xeb, 0x19, 0x17, 0x66, 0x9d, 0xd8, 0x01];

  #[test]
  fn quotes_a_path_with_spaces() {
    assert_eq!(quote_path(r"C:\Program Files\Ninja\ninja.exe"), INSTALLED);
  }

  #[test]
  fn is_enabled_when_the_entry_names_this_executable() {
    assert!(is_enabled_for(Some(INSTALLED), INSTALLED, None));
    assert!(is_enabled_for(Some(INSTALLED), INSTALLED, Some(APPROVED)));
  }

  #[test]
  fn is_enabled_regardless_of_path_case() {
    let upper = INSTALLED.to_ascii_uppercase();

    assert!(is_enabled_for(Some(&upper), INSTALLED, None));
  }

  #[test]
  fn is_disabled_when_the_entry_names_another_executable() {
    assert!(!is_enabled_for(Some(DEV_BUILD), INSTALLED, None));
  }

  #[test]
  fn is_disabled_when_there_is_no_entry() {
    assert!(!is_enabled_for(None, INSTALLED, None));
  }

  #[test]
  fn is_disabled_when_switched_off_in_task_manager() {
    assert!(!is_enabled_for(Some(INSTALLED), INSTALLED, Some(DISAPPROVED)));
  }

  #[test]
  fn is_disabled_when_the_task_manager_record_is_empty() {
    assert!(!is_enabled_for(Some(INSTALLED), INSTALLED, Some(&[])));
  }
}
