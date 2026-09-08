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
        // SAFETY: All-zeroes is not a valid `ThreadBound`, whose
        // retained pointer is non-null. Sound only while the mock
        // stays an opaque placeholder and none of its methods are
        // called, which is what the docs above require.
        unsafe { std::mem::zeroed() },
        // SAFETY: As above, the owning `Application` is never read from a
        // mock window.
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
      // SAFETY: All-zeroes is not a valid `Display`, since it holds an
      // `Arc` whose pointer is non-null. This holds only while the mock is
      // treated as an opaque placeholder and none of its methods are
      // called, which is what this method's docs require.
      #[allow(invalid_value)]
      inner: unsafe { std::mem::zeroed() },
    }
  }
}
