use windows::{
  core::{w, PCWSTR},
  Win32::{
    Foundation::{
      CloseHandle, GetLastError, ERROR_ALREADY_EXISTS,
      ERROR_FILE_NOT_FOUND, HANDLE,
    },
    System::Threading::{
      CreateMutexW, OpenMutexW, ReleaseMutex,
      SYNCHRONIZATION_ACCESS_RIGHTS,
    },
  },
};

/// Arbitrary GUID to uniquely identify the application.
///
/// Deliberately in the `Local\` namespace rather than `Global\`. `Local\`
/// scopes the mutex to the current login session, which means:
///
/// - Another user on the machine can't squat this name to permanently
///   block us from starting.
/// - Two users on a shared or Remote Desktop machine can each run their
///   own instance, instead of the first one blocking the second.
const APP_GUID: PCWSTR = w!("Local\\325d0ed7-7f60-4925-8d1b-aa287b26b218");

/// Platform-specific implementation of [`SingleInstance`].
pub struct SingleInstance {
  handle: HANDLE,
}

impl SingleInstance {
  /// Implements [`SingleInstance::new`].
  pub(crate) fn new() -> crate::Result<Self> {
    // Create a named mutex scoped to the current login session.
    let handle = unsafe { CreateMutexW(None, true, APP_GUID) }?;

    // Read the last error immediately, before anything else can overwrite
    // it. `CreateMutexW` succeeds either way, and reports a pre-existing
    // mutex through `ERROR_ALREADY_EXISTS`.
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

    if already_exists {
      // Close our handle so that we don't hold a reference to a mutex
      // owned by the other instance.
      unsafe {
        let _ = CloseHandle(handle);
      }

      return Err(crate::Error::Platform(
        "Another instance of the application is already running."
          .to_string(),
      ));
    }

    Ok(Self { handle })
  }

  /// Implements [`SingleInstance::is_running`].
  #[must_use]
  pub(crate) fn is_running() -> bool {
    // No access rights are requested, since only the mutex's existence
    // matters here.
    let res = unsafe {
      OpenMutexW(SYNCHRONIZATION_ACCESS_RIGHTS::default(), false, APP_GUID)
    };

    match res {
      // The mutex exists, so another instance is running. Close the handle
      // we just opened, since we only needed its existence.
      Ok(handle) => {
        unsafe {
          let _ = CloseHandle(handle);
        }
        true
      }
      // The only error meaning "not running" is a missing mutex. Anything
      // else (e.g. access denied) means it exists but we can't open it, so
      // fail safe and report it as running.
      Err(err) => err != ERROR_FILE_NOT_FOUND.into(),
    }
  }
}

impl Drop for SingleInstance {
  fn drop(&mut self) {
    unsafe {
      let _ = ReleaseMutex(self.handle);
      let _ = CloseHandle(self.handle);
    }
  }
}
