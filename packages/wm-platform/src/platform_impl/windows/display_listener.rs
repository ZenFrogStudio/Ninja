use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc,
};

use tokio::sync::mpsc;
use tracing::warn;
use windows::Win32::{
  Foundation::HANDLE,
  System::{
    Power::{
      RegisterPowerSettingNotification,
      UnregisterPowerSettingNotification, HPOWERNOTIFY,
      POWERBROADCAST_SETTING,
    },
    SystemServices::GUID_CONSOLE_DISPLAY_STATE,
  },
  UI::WindowsAndMessaging::{
    DBT_DEVNODES_CHANGED, DEVICE_NOTIFY_WINDOW_HANDLE,
    PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND,
    PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE, SPI_SETWORKAREA,
    WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_POWERBROADCAST,
    WM_SETTINGCHANGE,
  },
};

use crate::{Dispatcher, DispatcherExtWindows};

/// Value of `GUID_CONSOLE_DISPLAY_STATE` meaning the displays are off.
///
/// The other states are `1` (on) and `2` (dimmed). Only a full power-down
/// is treated as "off"; a dimmed display is still attached and reporting
/// its real bounds.
const DISPLAY_STATE_OFF: u8 = 0;

/// What a `WM_POWERBROADCAST` message means for display handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerEvent {
  /// The system is suspending. Display messages until it resumes describe
  /// a teardown, not the user's real monitor setup.
  Suspending,

  /// The system has resumed, by any route.
  Resumed,

  /// Not relevant to display handling.
  Unrelated,
}

/// Classifies a `WM_POWERBROADCAST` `WPARAM`.
///
/// Every resume variant is treated alike, including
/// `PBT_APMRESUMECRITICAL`, which is sent when the machine comes back from
/// an unexpected power loss. Missing any one of them leaves the suspend
/// flag latched on and display changes ignored for the rest of the
/// process' life.
fn classify_power_broadcast(wparam: u32) -> PowerEvent {
  match wparam {
    PBT_APMSUSPEND => PowerEvent::Suspending,
    PBT_APMRESUMEAUTOMATIC
    | PBT_APMRESUMESUSPEND
    | PBT_APMRESUMECRITICAL => PowerEvent::Resumed,
    _ => PowerEvent::Unrelated,
  }
}

/// Reads the display power state out of a `PBT_POWERSETTINGCHANGE`
/// message.
///
/// Returns `None` if the message is for a different power setting, or if
/// the pointer is null.
///
/// # Safety
///
/// `lparam` must be the `LPARAM` of a `WM_POWERBROADCAST` message whose
/// `WPARAM` is `PBT_POWERSETTINGCHANGE`, as documented for that message.
unsafe fn display_state_from_message(lparam: isize) -> Option<u8> {
  let setting = (lparam as *const POWERBROADCAST_SETTING).as_ref()?;

  if setting.PowerSetting != GUID_CONSOLE_DISPLAY_STATE {
    return None;
  }

  // `Data` is a variable-length array; this setting always carries a
  // single `DWORD`, whose low byte is the state.
  (setting.DataLength >= 1).then(|| setting.Data[0])
}

/// Platform-specific implementation of [`DisplayListener`].
pub(crate) struct DisplayListener {
  callback_id: Option<usize>,
  dispatcher: Dispatcher,
  power_notify: Option<HPOWERNOTIFY>,
}

impl DisplayListener {
  /// Implements [`DisplayListener::new`].
  pub(crate) fn new(
    event_tx: mpsc::UnboundedSender<()>,
    dispatcher: &Dispatcher,
  ) -> crate::Result<Self> {
    let is_system_suspended = Arc::new(AtomicBool::new(false));
    let is_display_off = Arc::new(AtomicBool::new(false));

    let callback_id = dispatcher.register_wndproc_callback(Box::new({
      let is_system_suspended = is_system_suspended.clone();
      let is_display_off = is_display_off.clone();

      move |_hwnd, message, wparam, lparam| {
        match message {
          WM_POWERBROADCAST => {
            #[allow(clippy::cast_possible_truncation)]
            let wparam = wparam as u32;

            if wparam == PBT_POWERSETTINGCHANGE {
              if let Some(state) =
                // SAFETY: `lparam` points to a `POWERBROADCAST_SETTING`
                // for the duration of the message, as documented for
                // `PBT_POWERSETTINGCHANGE`.
                unsafe { display_state_from_message(lparam) }
              {
                let is_off = state == DISPLAY_STATE_OFF;
                let was_off =
                  is_display_off.swap(is_off, Ordering::Relaxed);

                // Logged on every notification, not just transitions.
                // Windows sends the current state once on registration,
                // which is the only confirmation that the registration
                // took effect at all.
                tracing::info!(
                  "Display power state: {} ({state}).",
                  match state {
                    DISPLAY_STATE_OFF => "off",
                    1 => "on",
                    2 => "dimmed",
                    _ => "unknown",
                  }
                );

                // Display messages received while the panels were off were
                // dropped and are never replayed, so re-sync once now that
                // the real topology is back.
                if was_off && !is_off {
                  let _ = event_tx.send(());
                }
              }

              return Some(0);
            }

            match classify_power_broadcast(wparam) {
              PowerEvent::Suspending => {
                tracing::info!(
                  "System suspending; display changes will be ignored."
                );

                is_system_suspended.store(true, Ordering::Relaxed);
              }
              PowerEvent::Resumed => {
                tracing::info!(
                  "System resumed ({wparam}); resuming display changes."
                );

                is_system_suspended.store(false, Ordering::Relaxed);
              }
              PowerEvent::Unrelated => {}
            }

            Some(0)
          }
          WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DEVICECHANGE => {
            // Ignore display changes while the system is suspended or the
            // displays are powered down. Neither state reports the
            // monitor layout the user actually has.
            let ignore_reason =
              if is_system_suspended.load(Ordering::Relaxed) {
                Some("system suspended")
              } else if is_display_off.load(Ordering::Relaxed) {
                Some("displays powered off")
              } else {
                None
              };

            if let Some(reason) = ignore_reason {
              tracing::info!(
                "Display message {message} dropped: {reason}."
              );
              return Some(0);
            }

            #[allow(clippy::cast_possible_truncation)]
            let should_emit = match message {
              // Received when displays are connected and disconnected,
              // resolution changes, or arrangement changes.
              WM_DISPLAYCHANGE => true,
              // Received when the working area has changed. Fires when
              // the Windows taskbar is changed or an appbar is
              // registered or changed. 3rd-party apps like
              // ButteryTaskbar can trigger this message by calling
              // `SystemParametersInfo(SPI_SETWORKAREA, ...)`.
              WM_SETTINGCHANGE => wparam as u32 == SPI_SETWORKAREA.0,
              // Received when any device is connected or disconnected
              // (including non-display devices).
              // TODO: Check if this is actually needed. Previous C#
              // implementation did not use this.
              WM_DEVICECHANGE => wparam as u32 == DBT_DEVNODES_CHANGED,
              _ => unreachable!(),
            };

            if should_emit {
              let _ = event_tx.send(());
            }

            Some(0)
          }
          _ => None,
        }
      }
    }))?;

    // Ask Windows to report when the displays power down and come back.
    // Without this there is no way to tell an idle-timeout blank (which
    // can drop the video link and make every monitor disappear) apart
    // from the user actually unplugging their monitors.
    //
    // Registered after the callback so that the current state, which
    // Windows sends immediately on registration, isn't missed.
    let display_state_guid = GUID_CONSOLE_DISPLAY_STATE;

    // SAFETY: The handle is the event loop's message window, which
    // outlives this listener, and `display_state_guid` is a live local
    // that the OS only reads during the call.
    let power_notify = unsafe {
      RegisterPowerSettingNotification(
        HANDLE(dispatcher.message_window_handle() as *mut std::ffi::c_void),
        &raw const display_state_guid,
        DEVICE_NOTIFY_WINDOW_HANDLE,
      )
    }?;

    Ok(Self {
      callback_id: Some(callback_id),
      dispatcher: dispatcher.clone(),
      power_notify: Some(power_notify),
    })
  }

  /// Implements [`DisplayListener::terminate`].
  pub(crate) fn terminate(&mut self) -> crate::Result<()> {
    if let Some(id) = self.callback_id.take() {
      self.dispatcher.deregister_wndproc_callback(id)?;
    }

    if let Some(power_notify) = self.power_notify.take() {
      // SAFETY: The registration was made in `new` and is owned by this
      // listener. `take` clears it first, so it is unregistered once.
      unsafe { UnregisterPowerSettingNotification(power_notify) }?;
    }

    Ok(())
  }
}

impl Drop for DisplayListener {
  fn drop(&mut self) {
    if let Err(err) = self.terminate() {
      warn!("Failed to terminate display listener: {}", err);
    }
  }
}

#[cfg(test)]
mod tests {
  use windows::Win32::UI::WindowsAndMessaging::{
    PBT_APMQUERYSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL,
    PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
  };

  use super::{classify_power_broadcast, PowerEvent};

  #[test]
  fn suspend_is_classified_as_suspending() {
    assert_eq!(
      classify_power_broadcast(PBT_APMSUSPEND),
      PowerEvent::Suspending
    );
  }

  /// `PBT_APMRESUMECRITICAL` is the one that used to fall through, which
  /// left the suspend flag latched on and display changes ignored forever.
  #[test]
  fn every_resume_variant_is_classified_as_resumed() {
    for wparam in [
      PBT_APMRESUMEAUTOMATIC,
      PBT_APMRESUMESUSPEND,
      PBT_APMRESUMECRITICAL,
    ] {
      assert_eq!(
        classify_power_broadcast(wparam),
        PowerEvent::Resumed,
        "wparam {wparam} should be a resume"
      );
    }
  }

  #[test]
  fn unrelated_broadcasts_leave_the_flag_alone() {
    assert_eq!(
      classify_power_broadcast(PBT_APMQUERYSUSPEND),
      PowerEvent::Unrelated
    );
  }
}
