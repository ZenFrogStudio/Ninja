use std::{
  ptr::NonNull,
  sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use objc2_application_services::{AXError, AXObserver, AXUIElement};
use objc2_core_foundation::{
  kCFRunLoopDefaultMode, CFRetained, CFRunLoop, CFRunLoopSource, CFString,
};
use tokio::sync::mpsc;

use crate::{
  platform_impl::{
    Application, NativeWindow, ProcessId, WindowEventNotificationInner,
  },
  NativeWindowExtMacOs, ThreadBound, WindowEvent, WindowId,
};

/// Notifications to register for the `AXUIElement` of an application.
const AX_APP_NOTIFICATIONS: &[&str] =
  &["AXFocusedWindowChanged", "AXWindowCreated"];

/// Notifications to register for the `AXUIElement` of a window.
const AX_WINDOW_NOTIFICATIONS: &[&str] = &[
  "AXTitleChanged",
  "AXUIElementDestroyed",
  "AXWindowMoved",
  "AXWindowResized",
  "AXWindowDeminiaturized",
  "AXWindowMiniaturized",
];

/// Locks a list of application windows, recovering from a poisoned mutex.
///
/// The mutex guards a plain vector, so a panic elsewhere leaves the list
/// itself intact. Recovering keeps window events flowing instead of
/// poisoning every later observer callback.
fn lock_windows(
  app_windows: &Mutex<Vec<crate::NativeWindow>>,
) -> MutexGuard<'_, Vec<crate::NativeWindow>> {
  app_windows.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Context passed to the application event callback.
#[derive(Debug)]
struct ApplicationEventContext {
  application: Application,
  events_tx: mpsc::UnboundedSender<WindowEvent>,
  app_windows: Arc<Mutex<Vec<crate::NativeWindow>>>,
  observer: CFRetained<AXObserver>,
}

/// Represents an accessibility observer for a specific application.
#[derive(Debug)]
pub(crate) struct ApplicationObserver {
  pub(crate) pid: ProcessId,
  app_windows: Arc<Mutex<Vec<crate::NativeWindow>>>,
  events_tx: mpsc::UnboundedSender<WindowEvent>,
  _observer: CFRetained<AXObserver>,
  observer_source: CFRetained<CFRunLoopSource>,
}

// TODO: Remove this.
// SAFETY: `AXObserver` and `CFRunLoopSource` use atomic reference counts,
// and the only cross-thread use is dropping the observer, which just
// invalidates the source. Core Foundation documents that as thread-safe.
// The notification callback always runs on the run loop it was
// registered on.
unsafe impl Send for ApplicationObserver {}

impl ApplicationObserver {
  /// Creates a new `ApplicationObserver` for the given application.
  ///
  /// If `is_startup` is `true`, the observer will not emit
  /// `WindowEvent::Shown` for windows already running on startup.
  pub fn new(
    app: &Application,
    events_tx: mpsc::UnboundedSender<WindowEvent>,
    is_startup: bool,
  ) -> crate::Result<Self> {
    // SAFETY: `window_event_callback` has the signature that
    // `AXObserverCallback` expects, and `CFRetained::retain` is only
    // reached with a non-null pointer that `AXObserver::create` reported
    // as successfully created.
    let observer = unsafe {
      let mut observer = std::ptr::null_mut();

      let result = AXObserver::create(
        app.pid,
        Some(Self::window_event_callback),
        // SAFETY: Stack address of `observer` is guaranteed to be
        // non-null.
        NonNull::new(&raw mut observer).unwrap(),
      );

      if result != AXError::Success {
        return Err(crate::Error::Accessibility(
          "AXObserverCreate".to_string(),
          result.0,
        ));
      }

      CFRetained::retain(NonNull::new(observer).ok_or_else(|| {
        crate::Error::InvalidPointer("AXObserver is null.".to_string())
      })?)
    };

    let app_windows = Arc::new(Mutex::new(app.windows()?));
    let context = Box::into_raw(Box::new(ApplicationEventContext {
      application: app.clone(),
      events_tx: events_tx.clone(),
      app_windows: app_windows.clone(),
      observer: observer.clone(),
    }));

    let runloop =
      CFRunLoop::current().ok_or(crate::Error::EventLoopStopped)?;

    // SAFETY: `observer` is a live `AXObserver` retained by this scope,
    // so the run loop source it owns (`Get` rule) is valid here.
    let observer_source = unsafe { observer.run_loop_source() };
    // SAFETY: `kCFRunLoopDefaultMode` is a Core Foundation extern static
    // that is never mutated and stays alive for the lifetime of the
    // process.
    runloop.add_source(Some(&observer_source), unsafe {
      kCFRunLoopDefaultMode
    });

    // Register for all window notifications.
    // TODO: Remove from runloop if registration fails.
    Self::register_app_notifications(app, &observer, context)?;

    // Emit `WindowEvent::Shown` for all existing windows.
    for window in lock_windows(&app_windows).iter() {
      if let Err(err) =
        Self::register_window_notifications(window, &observer, context)
      {
        tracing::warn!(
          "Failed to register window notifications for PID {}: {}",
          app.pid,
          err
        );
      }

      // Don't emit `WindowEvent::Shown` for windows that are already
      // running on startup.
      if !is_startup {
        if let Err(err) = events_tx.send(WindowEvent::Shown {
          window: window.clone(),
          notification: crate::WindowEventNotification(None),
        }) {
          tracing::warn!(
            "Failed to send window event for PID {}: {}",
            app.pid,
            err
          );
        }
      }
    }

    Ok(Self {
      pid: app.pid,
      app_windows,
      events_tx,
      _observer: observer,
      observer_source,
    })
  }

  fn register_app_notifications(
    app: &Application,
    observer: &CFRetained<AXObserver>,
    context: *mut ApplicationEventContext,
  ) -> crate::Result<()> {
    for notification in AX_APP_NOTIFICATIONS {
      // SAFETY: `app.ax_element` is a live `AXUIElement`, and `context`
      // points to the `ApplicationEventContext` leaked in `new`, so it
      // stays valid for as long as the notification is registered.
      unsafe {
        let notification_cfstr = CFString::from_static_str(notification);
        let result = observer.add_notification(
          app.ax_element.get_ref()?,
          &notification_cfstr,
          context.cast::<std::ffi::c_void>(),
        );

        if result != AXError::Success {
          return Err(crate::Error::Platform(format!(
            "Failed to add notification {} for PID {}: {:?}",
            notification, app.pid, result
          )));
        }
      }
    }

    Ok(())
  }

  fn register_window_notifications(
    window: &crate::NativeWindow,
    observer: &CFRetained<AXObserver>,
    context: *mut ApplicationEventContext,
  ) -> crate::Result<()> {
    for notification in AX_WINDOW_NOTIFICATIONS {
      // SAFETY: The window's `AXUIElement` is live, and `context` points
      // to the `ApplicationEventContext` leaked in `new`, so it stays
      // valid for as long as the notification is registered.
      unsafe {
        let notification_cfstr = CFString::from_static_str(notification);
        let result = observer.add_notification(
          window.ax_ui_element().get_ref()?,
          &notification_cfstr,
          context.cast::<std::ffi::c_void>(),
        );

        if result != AXError::Success {
          return Err(crate::Error::Platform(format!(
            "Failed to add notification {} for window {}: {:?}",
            notification,
            window.id().0,
            result
          )));
        }
      }
    }

    Ok(())
  }

  pub(crate) fn emit_all_windows_destroyed(&self) {
    for window in lock_windows(&self.app_windows).iter() {
      if let Err(err) = self.events_tx.send(WindowEvent::Destroyed {
        window_id: window.id(),
        notification: crate::WindowEventNotification(None),
      }) {
        tracing::warn!(
          "Failed to send window event for PID {}: {}",
          self.pid,
          err
        );
      }
    }
  }

  pub(crate) fn emit_all_windows_hidden(&self) {
    for window in lock_windows(&self.app_windows).iter() {
      if let Err(err) = self.events_tx.send(WindowEvent::Hidden {
        window: window.clone(),
        notification: crate::WindowEventNotification(None),
      }) {
        tracing::warn!(
          "Failed to send window event for PID {}: {}",
          self.pid,
          err
        );
      }
    }
  }

  pub(crate) fn emit_all_windows_shown(&self) {
    for window in lock_windows(&self.app_windows).iter() {
      if let Err(err) = self.events_tx.send(WindowEvent::Shown {
        window: window.clone(),
        notification: crate::WindowEventNotification(None),
      }) {
        tracing::warn!(
          "Failed to send window event for PID {}: {}",
          self.pid,
          err
        );
      }
    }
  }

  /// Callback function for accessibility window events.
  ///
  /// # Safety
  ///
  /// `context` must be the `ApplicationEventContext` pointer registered
  /// with `AXObserver::add_notification`, and `element` must be the live
  /// `AXUIElement` that the accessibility API passes in. Only the
  /// observer's run loop may invoke this.
  #[allow(clippy::too_many_lines)]
  unsafe extern "C-unwind" fn window_event_callback(
    _observer: NonNull<AXObserver>,
    element: NonNull<AXUIElement>,
    notification_name: NonNull<CFString>,
    context: *mut std::ffi::c_void,
  ) {
    if context.is_null() {
      tracing::error!("Window event callback received null context.");
      return;
    }

    // SAFETY: `context` is the leaked `ApplicationEventContext` per this
    // function's contract, checked non-null above. Callbacks are
    // serialised on the observer's run loop, so the borrow is unaliased.
    let context = &mut *context.cast::<ApplicationEventContext>();
    // SAFETY: `element` is the live `AXUIElement` passed in by the
    // accessibility API, which follows the `Get` rule, so it is retained
    // here to keep it alive beyond the callback.
    let ax_element = unsafe { CFRetained::retain(element) };
    let notification = WindowEventNotificationInner {
      name: notification_name.as_ref().to_string(),
      ax_element_ptr: element.as_ptr().cast::<std::ffi::c_void>(),
    };

    tracing::debug!(
      "Received window event: {} for PID: {}",
      notification.name,
      context.application.pid
    );

    let found_window = {
      let app_windows = lock_windows(&context.app_windows);

      app_windows
        .iter()
        .find(|window| {
          window.ax_ui_element().get_ref().ok() == Some(&ax_element)
        })
        .cloned()
    };

    if notification.name.as_str() == "AXUIElementDestroyed" {
      if let Some(window) = &found_window {
        lock_windows(&context.app_windows)
          .retain(|w| w.id() != window.id());

        if let Err(err) = context.events_tx.send(WindowEvent::Destroyed {
          window_id: window.id(),
          notification: crate::WindowEventNotification(Some(notification)),
        }) {
          tracing::warn!(
            "Failed to send window event for PID {}: {}",
            context.application.pid,
            err
          );
        }
      }

      return;
    }

    let is_new_window = found_window.is_none();
    let window = found_window.unwrap_or_else(|| {
      let window_id = WindowId::from_window_element(&ax_element);
      let ax_element = ThreadBound::new(
        ax_element,
        context.application.dispatcher.clone(),
      );
      NativeWindow::new(window_id, ax_element, context.application.clone())
        .into()
    });

    if is_new_window {
      lock_windows(&context.app_windows).push(window.clone());
      let _ = Self::register_window_notifications(
        &window,
        &context.observer.clone(),
        context,
      );

      if let Err(err) = context.events_tx.send(WindowEvent::Shown {
        window: window.clone(),
        notification: crate::WindowEventNotification(Some(
          notification.clone(),
        )),
      }) {
        tracing::warn!(
          "Failed to send window event for PID {}: {}",
          context.application.pid,
          err
        );
      }
    }

    let window_event = match notification.name.as_str() {
      "AXFocusedWindowChanged" => WindowEvent::Focused {
        window,
        notification: crate::WindowEventNotification(Some(notification)),
      },
      "AXWindowMoved" | "AXWindowResized" => WindowEvent::MovedOrResized {
        window,
        is_interactive_start: false,
        is_interactive_end: false,
        notification: crate::WindowEventNotification(Some(notification)),
      },
      "AXWindowMiniaturized" => WindowEvent::Minimized {
        window,
        notification: crate::WindowEventNotification(Some(notification)),
      },
      "AXWindowDeminiaturized" => WindowEvent::MinimizeEnded {
        window,
        notification: crate::WindowEventNotification(Some(notification)),
      },
      "AXTitleChanged" => WindowEvent::TitleChanged {
        window,
        notification: crate::WindowEventNotification(Some(notification)),
      },
      _ => {
        tracing::debug!(
          "Unhandled window notification: {} for PID: {}",
          notification.name,
          context.application.pid
        );
        return;
      }
    };

    if let Err(err) = context.events_tx.send(window_event) {
      tracing::warn!(
        "Failed to send window event for PID {}: {}",
        context.application.pid,
        err
      );
    }
  }
}

impl Drop for ApplicationObserver {
  fn drop(&mut self) {
    // Invalidate the runloop source. This is thread-safe and is OK to call
    // after the run loop is stopped.
    self.observer_source.invalidate();
  }
}
