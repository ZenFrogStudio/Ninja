use std::sync::{
  atomic::{AtomicBool, Ordering},
  mpsc, Arc,
};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_core_foundation::{
  kCFRunLoopDefaultMode, CFRetained, CFRunLoop, CFRunLoopSource,
  CFRunLoopSourceContext,
};

use crate::{DispatchFn, Dispatcher};

/// Source for dispatching callbacks onto the event loop thread.
#[derive(Clone)]
pub(crate) struct EventLoopSource {
  dispatch_tx: mpsc::Sender<Box<DispatchFn>>,
  source: CFRetained<CFRunLoopSource>,
  run_loop: CFRetained<CFRunLoop>,
  pub(crate) thread_id: std::thread::ThreadId,
}

impl EventLoopSource {
  /// Creates a source for dispatching callbacks onto the current thread's
  /// run loop.
  ///
  /// The source is not scheduled on the run loop yet, so `schedule` must
  /// be called before dispatched callbacks are able to run.
  fn new() -> crate::Result<Self> {
    let (dispatch_tx, dispatch_rx) = mpsc::channel();
    let dispatch_rx_ptr =
      Box::into_raw(Box::new(dispatch_rx)).cast::<std::ffi::c_void>();

    // Create `CFRunLoopSource` context.
    let mut context = CFRunLoopSourceContext {
      version: 0,
      info: dispatch_rx_ptr,
      retain: None,
      release: Some(EventLoop::runloop_source_released_callback),
      copyDescription: None,
      equal: None,
      hash: None,
      schedule: None,
      cancel: None,
      perform: Some(EventLoop::runloop_signaled_callback),
    };

    // SAFETY: The receiver behind `info` stays alive until the source is
    // released, at which point the release callback frees it.
    let source =
      unsafe { CFRunLoopSource::new(None, 0, &raw mut context) }.ok_or(
        crate::Error::Platform(
          "Failed to create run loop source.".to_string(),
        ),
      )?;

    let run_loop =
      CFRunLoop::current().ok_or(crate::Error::EventLoopStopped)?;

    Ok(Self {
      dispatch_tx,
      source,
      run_loop,
      thread_id: std::thread::current().id(),
    })
  }

  /// Creates an inert source for use in tests.
  ///
  /// The source is never scheduled on a run loop, so cross-thread
  /// dispatches are queued but never run. Dispatches from the creating
  /// thread run inline.
  // LINT: Only used by `test_utils`, which the `src/test.rs` target does
  // not compile.
  #[cfg(feature = "test_utils")]
  #[allow(dead_code)]
  pub(crate) fn mock() -> crate::Result<Self> {
    Self::new()
  }

  /// Schedules the source on its run loop, so that dispatched callbacks
  /// are able to run.
  fn schedule(&self) {
    // SAFETY: `kCFRunLoopDefaultMode` is a Core Foundation extern static
    // that is never mutated and stays alive for the lifetime of the
    // process.
    self
      .run_loop
      .add_source(Some(&self.source), unsafe { kCFRunLoopDefaultMode });
  }

  pub(crate) fn send_dispatch_async<F>(
    &self,
    dispatch_fn: F,
  ) -> crate::Result<()>
  where
    F: FnOnce() + Send + 'static,
  {
    // TODO: Avoid duplicate check in `dispatch_sync`.
    if std::thread::current().id() == self.thread_id {
      dispatch_fn();
      return Ok(());
    }

    self
      .dispatch_tx
      .send(Box::new(dispatch_fn))
      .map_err(|_| crate::Error::ChannelSend)?;

    // Signal the run loop source, which schedules the `perform` callback
    // to be invoked. If signaled multiple times in a short period, this
    // gets coalesced into a single signal.
    self.source.signal();

    // Wake up the run loop to process the signal.
    self.run_loop.wake_up();
    Ok(())
  }

  pub(crate) fn send_dispatch_sync<F>(
    &self,
    dispatch_fn: F,
  ) -> crate::Result<()>
  where
    F: FnOnce() + Send,
  {
    // SAFETY: Usage of this function needs to be in a synchronous
    // context where the dispatch function will be executed before the
    // caller's stack frame is dropped.
    let dispatch_fn_static = unsafe {
      std::mem::transmute::<
        Box<dyn FnOnce() + Send>,
        Box<dyn FnOnce() + Send + 'static>,
      >(Box::new(dispatch_fn))
    };

    self.send_dispatch_async(dispatch_fn_static)
  }

  pub(crate) fn send_stop(&self) -> crate::Result<()> {
    let (result_tx, result_rx) = std::sync::mpsc::channel();

    self.send_dispatch_sync(|| {
      // SAFETY: This closure either runs inline on the thread that
      // created the source, or via the source scheduled by
      // `add_dispatch_source`. Both are the main thread.
      let mtm = unsafe { MainThreadMarker::new_unchecked() };

      // Call `stop()` to mark the run loop for termination.
      let ns_app = NSApplication::sharedApplication(mtm);
      ns_app.stop(None);

      // `stop()` only takes effect after processing a subsequent UI event.
      // Post a dummy event so the application actually exits.
      ns_app.abortModal();

      let _ = result_tx.send(());
    })?;

    result_rx
      .recv_timeout(std::time::Duration::from_secs(3))
      .map_err(crate::Error::ChannelRecv)
  }
}

// SAFETY: `CFRunLoop` and `CFRunLoopSource` are thread-safe types. The
// `objc2` bindings don't implement `Send + Sync`.
unsafe impl Send for EventLoopSource {}
// SAFETY: Signalling the source and waking the run loop are documented as
// thread-safe, and the remaining fields are shareable across threads.
unsafe impl Sync for EventLoopSource {}

/// Platform-specific implementation of [`EventLoop`].
pub(crate) struct EventLoop {
  source: EventLoopSource,
  stopped: Arc<AtomicBool>,
}

impl EventLoop {
  /// Implements [`EventLoop::new`].
  pub fn new() -> crate::Result<(Self, Dispatcher)> {
    // Add a new run loop source that allows dispatching from any thread.
    let source = Self::add_dispatch_source()?;

    let stopped = Arc::new(AtomicBool::new(false));
    let dispatcher = Dispatcher::new(source.clone(), stopped.clone());

    Ok((
      Self {
        source: source.clone(),
        stopped,
      },
      dispatcher,
    ))
  }

  /// Implements [`EventLoop::run`].
  #[allow(clippy::unused_self)]
  pub fn run(self) -> crate::Result<()> {
    let mtm =
      MainThreadMarker::new().ok_or(crate::Error::NotMainThread)?;

    tracing::info!("Starting macOS event loop.");
    NSApplication::sharedApplication(mtm).run();

    tracing::info!("macOS event loop exiting.");
    Ok(())
  }

  /// Adds a source (`CFRunLoopSource`) for allowing dispatches to
  /// the current run loop.
  ///
  /// Can only be called on the main thread.
  pub(crate) fn add_dispatch_source() -> crate::Result<EventLoopSource> {
    let mtm =
      MainThreadMarker::new().ok_or(crate::Error::NotMainThread)?;

    // Initialize `NSApplication` on the main thread. This is necessary for
    // some AppKit components (e.g. system tray) to be functional.
    // TODO: Skip this if not on the main thread, and instead run a normal
    // run loop.
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let source = EventLoopSource::new()?;
    source.schedule();

    Ok(source)
  }

  // This function is called by the `CFRunLoopSource` when signaled.
  extern "C-unwind" fn runloop_signaled_callback(
    info: *mut std::ffi::c_void,
  ) {
    // SAFETY: `info` is the receiver pointer stored in the
    // `CFRunLoopSourceContext` in `EventLoopSource::new`. It is only
    // freed by the release callback, which Core Foundation runs after the
    // source is invalidated and can no longer perform.
    let callbacks =
      unsafe { &*(info as *const mpsc::Receiver<Box<DispatchFn>>) };

    // Process any pending dispatched callbacks. Multiple run loop signals
    // may be coalesced together (calling `perform` only once), so it's
    // important to drain all pending callbacks.
    for callback in callbacks.try_iter() {
      callback();
    }
  }

  // This function is called when the `CFRunLoopSource` is released.
  extern "C-unwind" fn runloop_source_released_callback(
    info: *const std::ffi::c_void,
  ) {
    // SAFETY: This pointer was created with `Box::into_raw` in
    // `add_dispatch_source`, so it can safely be converted back to a `Box`
    // and dropped.
    let _ = unsafe {
      Box::from_raw(info as *mut mpsc::Receiver<Box<DispatchFn>>)
    };
  }
}

impl Drop for EventLoop {
  fn drop(&mut self) {
    tracing::info!("Shutting down event loop.");

    // Stop the run loop if not already stopped.
    if !self.stopped.load(Ordering::SeqCst) {
      let _ = self.source.send_stop();
    }

    // Invalidate the runloop source to trigger its release callback. This
    // is thread-safe and is OK to call after the run loop is stopped.
    self.source.source.invalidate();
  }
}
