use std::cell::RefCell;

use anyhow::Context;
use windows::Win32::System::Com::{
  CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED,
};

thread_local! {
  /// Manages per-thread COM initialization. COM must be initialized on
  /// each thread that uses it, so the guard is stored in thread-local
  /// storage and dropped when the thread exits.
  static COM_INIT: RefCell<Option<ComInit>> = const { RefCell::new(None) };
}

/// Initializes COM on the current thread, if it isn't already initialized
/// by a previous call on this thread.
///
/// COM is uninitialized automatically when the thread exits.
pub fn init_com() -> anyhow::Result<()> {
  COM_INIT.with(|com_init| {
    let mut com_init = com_init.borrow_mut();

    if com_init.is_none() {
      *com_init = Some(ComInit::new()?);
    }

    Ok(())
  })
}

/// Guard that uninitializes COM on the current thread when dropped.
struct ComInit();

impl ComInit {
  /// Initializes COM on the current thread with multithreaded object
  /// concurrency.
  ///
  /// Fails if COM is already initialized on the thread with an
  /// incompatible threading model.
  fn new() -> anyhow::Result<Self> {
    // SAFETY: `CoInitializeEx` is safe to call from any thread, and the
    // returned guard pairs it with a `CoUninitialize` on the same thread.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
      .ok()
      .context("Unable to initialize COM.")?;

    Ok(Self())
  }
}

impl Drop for ComInit {
  fn drop(&mut self) {
    // SAFETY: Balances the `CoInitializeEx` call in `ComInit::new`, which
    // ran on this same thread since the guard lives in thread-local
    // storage.
    unsafe { CoUninitialize() };
  }
}
