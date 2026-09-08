use objc2::{
  define_class, msg_send, rc::Retained, runtime::AnyObject, sel,
  AnyThread, DefinedClass,
};
use objc2_app_kit::{
  NSApplicationDidChangeScreenParametersNotification,
  NSRunningApplication, NSWorkspace,
  NSWorkspaceActiveSpaceDidChangeNotification,
  NSWorkspaceDidActivateApplicationNotification,
  NSWorkspaceDidHideApplicationNotification,
  NSWorkspaceDidLaunchApplicationNotification,
  NSWorkspaceDidTerminateApplicationNotification,
  NSWorkspaceDidUnhideApplicationNotification,
  NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
  ns_string, NSNotification, NSNotificationCenter, NSNotificationName,
  NSObject, NSString,
};
use tokio::sync::mpsc;

/// Notification names for observing macOS workspace and screen events.
#[derive(Debug)]
pub(crate) enum NotificationName {
  WorkspaceActiveSpaceDidChange,
  WorkspaceDidActivateApplication,
  WorkspaceDidLaunchApplication,
  WorkspaceDidTerminateApplication,
  WorkspaceDidHideApplication,
  WorkspaceDidUnhideApplication,
  WorkspaceDidWake,
  WorkspaceWillSleep,
  ApplicationDidChangeScreenParameters,
}

impl From<&NSNotificationName> for NotificationName {
  fn from(name: &NSNotificationName) -> Self {
    // SAFETY: `NSWorkspaceDidLaunchApplicationNotification` is an AppKit
    // extern static that is never mutated and stays alive for the
    // lifetime of the process.
    if name == unsafe { NSWorkspaceDidLaunchApplicationNotification } {
      Self::WorkspaceDidLaunchApplication
    } else if name
      // SAFETY: `NSWorkspaceDidActivateApplicationNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      == unsafe { NSWorkspaceDidActivateApplicationNotification }
    {
      Self::WorkspaceDidActivateApplication
    } else if name
      // SAFETY: `NSWorkspaceDidTerminateApplicationNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      == unsafe { NSWorkspaceDidTerminateApplicationNotification }
    {
      Self::WorkspaceDidTerminateApplication
    } else if name
      // SAFETY: `NSWorkspaceActiveSpaceDidChangeNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      == unsafe { NSWorkspaceActiveSpaceDidChangeNotification }
    {
      Self::WorkspaceActiveSpaceDidChange
      // SAFETY: `NSWorkspaceDidHideApplicationNotification` is an AppKit
      // extern static, alive for the lifetime of the process.
    } else if name == unsafe { NSWorkspaceDidHideApplicationNotification }
    {
      Self::WorkspaceDidHideApplication
    } else if name
      // SAFETY: `NSWorkspaceDidUnhideApplicationNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      == unsafe { NSWorkspaceDidUnhideApplicationNotification }
    {
      Self::WorkspaceDidUnhideApplication
      // SAFETY: `NSWorkspaceDidWakeNotification` is an AppKit extern
      // static, alive for the lifetime of the process.
    } else if name == unsafe { NSWorkspaceDidWakeNotification } {
      Self::WorkspaceDidWake
      // SAFETY: `NSWorkspaceWillSleepNotification` is an AppKit extern
      // static, alive for the lifetime of the process.
    } else if name == unsafe { NSWorkspaceWillSleepNotification } {
      Self::WorkspaceWillSleep
    } else if name
      // SAFETY: `NSApplicationDidChangeScreenParametersNotification` is
      // an AppKit extern static, alive for the lifetime of the process.
      == unsafe { NSApplicationDidChangeScreenParametersNotification }
    {
      Self::ApplicationDidChangeScreenParameters
    } else {
      panic!("Unknown notification name: {name}");
    }
  }
}

impl From<NotificationName> for &NSString {
  fn from(name: NotificationName) -> Self {
    match name {
      // SAFETY: `NSWorkspaceActiveSpaceDidChangeNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      NotificationName::WorkspaceActiveSpaceDidChange => unsafe {
        NSWorkspaceActiveSpaceDidChangeNotification
      },
      // SAFETY: `NSWorkspaceDidActivateApplicationNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      NotificationName::WorkspaceDidActivateApplication => unsafe {
        NSWorkspaceDidActivateApplicationNotification
      },
      // SAFETY: `NSWorkspaceDidLaunchApplicationNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      NotificationName::WorkspaceDidLaunchApplication => unsafe {
        NSWorkspaceDidLaunchApplicationNotification
      },
      // SAFETY: `NSWorkspaceDidTerminateApplicationNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      NotificationName::WorkspaceDidTerminateApplication => unsafe {
        NSWorkspaceDidTerminateApplicationNotification
      },
      // SAFETY: `NSWorkspaceDidHideApplicationNotification` is an AppKit
      // extern static, alive for the lifetime of the process.
      NotificationName::WorkspaceDidHideApplication => unsafe {
        NSWorkspaceDidHideApplicationNotification
      },
      // SAFETY: `NSWorkspaceDidUnhideApplicationNotification` is an
      // AppKit extern static, alive for the lifetime of the process.
      NotificationName::WorkspaceDidUnhideApplication => unsafe {
        NSWorkspaceDidUnhideApplicationNotification
      },
      // SAFETY: `NSWorkspaceDidWakeNotification` is an AppKit extern
      // static, alive for the lifetime of the process.
      NotificationName::WorkspaceDidWake => unsafe {
        NSWorkspaceDidWakeNotification
      },
      // SAFETY: `NSWorkspaceWillSleepNotification` is an AppKit extern
      // static, alive for the lifetime of the process.
      NotificationName::WorkspaceWillSleep => unsafe {
        NSWorkspaceWillSleepNotification
      },
      // SAFETY: `NSApplicationDidChangeScreenParametersNotification` is
      // an AppKit extern static, alive for the lifetime of the process.
      NotificationName::ApplicationDidChangeScreenParameters => unsafe {
        NSApplicationDidChangeScreenParametersNotification
      },
    }
  }
}

/// Events received from macOS notification center observers.
#[derive(Debug)]
pub(crate) enum NotificationEvent {
  WorkspaceActiveSpaceDidChange,
  WorkspaceDidActivateApplication(Retained<NSRunningApplication>),
  WorkspaceDidLaunchApplication(Retained<NSRunningApplication>),
  WorkspaceDidTerminateApplication(Retained<NSRunningApplication>),
  WorkspaceDidHideApplication(Retained<NSRunningApplication>),
  WorkspaceDidUnhideApplication(Retained<NSRunningApplication>),
  WorkspaceWillSleep,
  WorkspaceDidWake,
  ApplicationDidChangeScreenParameters,
}

/// Instance variables for `NotificationObserver`.
#[repr(C)]
pub(crate) struct NotificationObserverIvars {
  events_tx: mpsc::UnboundedSender<NotificationEvent>,
}

define_class! {
  // SAFETY:
  // - The superclass `NSObject` does not have any subclassing requirements.
  // - `NotificationObserver` does not implement `Drop`.
  #[unsafe(super(NSObject))]
  #[ivars = Box<NotificationObserverIvars>]
  pub(crate) struct NotificationObserver;

  // SAFETY: Each of these method signatures must match their invocations.
  impl NotificationObserver {
    #[unsafe(method(onEvent:))]
    fn on_event(&self, notif: &NSNotification) {
      self.handle_event(notif);
    }
  }
}

impl NotificationObserver {
  pub fn new(
  ) -> (Retained<Self>, mpsc::UnboundedReceiver<NotificationEvent>) {
    let (events_tx, events_rx) = mpsc::unbounded_channel();

    let instance = Self::alloc()
      .set_ivars(Box::new(NotificationObserverIvars { events_tx }));

    // SAFETY: The signature of `NSObject`'s `init` method is correct.
    (unsafe { msg_send![super(instance), init] }, events_rx)
  }

  fn handle_event(&self, notif: &NSNotification) {
    tracing::debug!("Received notification: {notif:#?}");

    match NotificationName::from(&*notif.name()) {
      NotificationName::WorkspaceActiveSpaceDidChange => {
        self.emit_event(NotificationEvent::WorkspaceActiveSpaceDidChange);
      }
      NotificationName::WorkspaceDidActivateApplication => {
        // SAFETY: The notification is an activate notification, whose
        // `NSWorkspaceApplicationKey` entry AppKit documents as an
        // `NSRunningApplication`.
        if let Some(app) = unsafe { app_from_notification(notif) } {
          self.emit_event(
            NotificationEvent::WorkspaceDidActivateApplication(app),
          );
        } else {
          tracing::warn!(
            "Failed to extract application from activate notification"
          );
        }
      }
      NotificationName::WorkspaceDidLaunchApplication => {
        // SAFETY: The notification is a launch notification, whose
        // `NSWorkspaceApplicationKey` entry AppKit documents as an
        // `NSRunningApplication`.
        if let Some(app) = unsafe { app_from_notification(notif) } {
          self.emit_event(
            NotificationEvent::WorkspaceDidLaunchApplication(app),
          );
        } else {
          tracing::warn!(
            "Failed to extract application from launch notification"
          );
        }
      }
      NotificationName::WorkspaceDidTerminateApplication => {
        // SAFETY: The notification is a terminate notification, whose
        // `NSWorkspaceApplicationKey` entry AppKit documents as an
        // `NSRunningApplication`.
        if let Some(app) = unsafe { app_from_notification(notif) } {
          self.emit_event(
            NotificationEvent::WorkspaceDidTerminateApplication(app),
          );
        } else {
          tracing::warn!(
            "Failed to extract application from terminate notification"
          );
        }
      }
      NotificationName::WorkspaceDidHideApplication => {
        // SAFETY: The notification is a hide notification, whose
        // `NSWorkspaceApplicationKey` entry AppKit documents as an
        // `NSRunningApplication`.
        if let Some(app) = unsafe { app_from_notification(notif) } {
          self.emit_event(NotificationEvent::WorkspaceDidHideApplication(
            app,
          ));
        }
      }
      NotificationName::WorkspaceDidUnhideApplication => {
        // SAFETY: The notification is an unhide notification, whose
        // `NSWorkspaceApplicationKey` entry AppKit documents as an
        // `NSRunningApplication`.
        if let Some(app) = unsafe { app_from_notification(notif) } {
          self.emit_event(
            NotificationEvent::WorkspaceDidUnhideApplication(app),
          );
        }
      }
      NotificationName::WorkspaceDidWake => {
        self.emit_event(NotificationEvent::WorkspaceDidWake);
      }
      NotificationName::WorkspaceWillSleep => {
        self.emit_event(NotificationEvent::WorkspaceWillSleep);
      }
      NotificationName::ApplicationDidChangeScreenParameters => {
        self.emit_event(
          NotificationEvent::ApplicationDidChangeScreenParameters,
        );
      }
    }
  }

  fn emit_event(&self, event: NotificationEvent) {
    if let Err(err) = self.ivars().events_tx.send(event) {
      tracing::warn!("Failed to send event: {err}");
    }
  }
}

/// Wrapper around `NSNotificationCenter` for registering event observers.
#[derive(Debug)]
pub(crate) struct NotificationCenter {
  inner: Retained<NSNotificationCenter>,
}

impl NotificationCenter {
  pub fn workspace_center() -> Self {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();

    Self { inner: center }
  }

  pub fn default_center() -> Self {
    let center = NSNotificationCenter::defaultCenter();

    Self { inner: center }
  }

  /// Registers an observer for the given notification name.
  ///
  /// # Safety
  ///
  /// `object` must be a valid Objective-C object, and must stay alive for
  /// as long as it is used as the notification's sender.
  pub unsafe fn add_observer(
    &mut self,
    notification_name: NotificationName,
    observer: &NotificationObserver,
    object: Option<&AnyObject>,
  ) {
    tracing::info!("Adding observer for {notification_name:?}.");

    // SAFETY: `observer` is a `NotificationObserver`, which defines the
    // `onEvent:` method with a matching `&NSNotification` signature, and
    // `object` is a valid sender per this function's contract.
    self.inner.addObserver_selector_name_object(
      observer,
      sel!(onEvent:),
      Some(notification_name.into()),
      object,
    );
  }
}

/// Extracts the `NSRunningApplication` that a workspace notification is
/// about.
///
/// # Safety
///
/// The notification must be one of the `NSWorkspace` application
/// notifications (launch, activate, terminate, hide, or unhide), whose
/// `NSWorkspaceApplicationKey` value is an `NSRunningApplication`. The
/// cast is unchecked, so any other notification is undefined behaviour.
pub unsafe fn app_from_notification(
  notification: &NSNotification,
) -> Option<Retained<NSRunningApplication>> {
  // SAFETY: Per this function's contract, the value stored under
  // `NSWorkspaceApplicationKey` is an `NSRunningApplication`, so the
  // unchecked cast preserves the object's real class.
  notification
    .userInfo()?
    .objectForKey(ns_string!("NSWorkspaceApplicationKey"))
    .map(|app| Retained::<AnyObject>::cast_unchecked(app))
}
