//! Utilities for testing.
//!
//! Available via the `test_utils` Cargo feature.
use std::sync::{atomic::AtomicBool, Arc};

use crate::platform_impl;
#[cfg(target_os = "macos")]
pub use crate::WindowId;
pub use crate::{Dispatcher, Display, NativeWindow};

impl Dispatcher {
  /// Creates a mock `Dispatcher` for use in tests.
  ///
  /// The mock is backed by an event loop source that is never run, so
  /// closures dispatched from another thread never execute.
  ///
  /// # Panics
  ///
  /// Panics if the mock event loop source cannot be created.
  #[must_use]
  pub fn mock() -> Self {
    #[cfg(target_os = "windows")]
    let source = platform_impl::EventLoopSource::mock();
    #[cfg(target_os = "macos")]
    let source = platform_impl::EventLoopSource::mock()
      .expect("Failed to create a mock event loop source.");

    Self::new(source, Arc::new(AtomicBool::new(false)))
  }
}

impl NativeWindow {
  /// Creates a mock `NativeWindow` for use in tests.
  ///
  /// Calling any methods on the mock is undefined behavior and may panic.
  #[must_use]
  pub fn mock() -> Self {
    #[cfg(target_os = "windows")]
    {
      platform_impl::NativeWindow::new(0).into()
    }
    #[cfg(target_os = "macos")]
    {
      #[allow(invalid_value)]
      platform_impl::NativeWindow::new(
        WindowId(0),
        unsafe { std::mem::zeroed() },
        unsafe { std::mem::zeroed() },
      )
      .into()
    }
  }
}

impl Display {
  /// Creates a mock `Display` for use in tests.
  ///
  /// Calling any methods on the mock is undefined behavior and may panic.
  #[must_use]
  pub fn mock() -> Self {
    Self {
      #[cfg(target_os = "windows")]
      inner: platform_impl::Display::new(0),
      #[cfg(target_os = "macos")]
      #[allow(invalid_value)]
      inner: unsafe { std::mem::zeroed() },
    }
  }
}
