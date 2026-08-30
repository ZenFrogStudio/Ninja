use wm_common::try_warn;

use crate::{
  commands::monitor::{
    add_monitor, move_bounded_workspaces_to_new_monitor, remove_monitor,
    restore_workspaces_to_origin, sort_monitors, update_monitor,
  },
  models::{Monitor, NativeMonitorProperties},
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn handle_display_settings_changed(
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  tracing::info!("Display settings changed.");

  // Ignore the event if retrieval of the displays or their properties
  // fails (can happen transiently during sleep/wake).
  let displays = try_warn!(state
    .dispatcher
    .sorted_displays()
    .map_err(anyhow::Error::from)
    .and_then(|displays| {
      displays
        .into_iter()
        .map(|display| {
          let properties = NativeMonitorProperties::try_from(&display)?;
          Ok((display, properties))
        })
        .try_collect::<Vec<_>>()
    }));

  let mut pending_monitors = state.monitors();
  let mut unmatched_displays = Vec::new();

  // Match each display to an existing monitor and update it.
  for (display, properties) in displays {
    if let Some((monitor, index)) =
      find_matching_monitor(&pending_monitors, &properties)
    {
      update_monitor(monitor, &display, properties, state)?;
      pending_monitors.remove(index);
    } else {
      tracing::info!(
        "Display '{}' matched no existing monitor.",
        properties.device_name
      );

      unmatched_displays.push((display, properties));
    }
  }

  let mut new_monitors: Vec<Monitor> = Vec::new();

  // Pair unmatched displays with unmatched monitors, or add new ones.
  for (display, properties) in unmatched_displays {
    if pending_monitors.is_empty() {
      let monitor = add_monitor(display, properties, state)?;
      new_monitors.push(monitor);
    } else {
      let monitor = pending_monitors.remove(0);

      // A stored monitor is reused for a display it didn't match on,
      // without re-evaluating its bound workspaces.
      tracing::info!(
        "Pairing unmatched display '{}' with leftover monitor '{}'.",
        properties.device_name,
        monitor.native_properties().device_name
      );

      update_monitor(&monitor, &display, properties, state)?;
    }
  }

  // Remove monitors that no longer have a corresponding display and move
  // their workspaces to other monitors.
  //
  // Prevent removal of the last monitor (i.e. for when all monitors are
  // disconnected). This will cause the WM's monitors to temporarily
  // mismatch the OS monitor state, however, it'll be updated correctly
  // when a new monitor is connected again.
  for monitor in pending_monitors {
    if state.monitors().len() > 1 {
      remove_monitor(monitor, state, config)?;
    } else {
      tracing::info!(
        "Keeping monitor '{}': last monitor guard.",
        monitor.native_properties().device_name
      );
    }
  }

  // Sort monitors by position.
  sort_monitors(&state.root_container)?;

  for new_monitor in new_monitors {
    move_bounded_workspaces_to_new_monitor(&new_monitor, state, config)?;
  }

  // Send workspaces displaced by an earlier teardown back to the monitor
  // they came from, now that the bound ones have been placed. Covers
  // monitors that were re-paired above as well as newly added ones.
  restore_workspaces_to_origin(state, config)?;

  for window in state.windows() {
    // Display setting changes can spread windows out sporadically, so mark
    // all windows as needing a DPI adjustment (just in case).
    window.set_has_pending_dpi_adjustment(true);

    // Need to update floating position of moved windows when a monitor is
    // disconnected or if the primary display is changed. The primary
    // display dictates the position of 0,0.
    // Skip the window rather than aborting, so that one detached window
    // can't prevent the remaining windows from being adjusted or the
    // final redraw from being queued.
    let Some(workspace) = window.workspace() else {
      continue;
    };

    let should_recenter = if window.has_custom_floating_placement() {
      let workspace_rect = workspace.to_rect()?;

      // Keep the placement if it still intersects the workspace, since
      // `PlatformEvent::DisplaySettingsChanged` can be triggered by
      // non-monitor changes (e.g. unplugging a USB device).
      window
        .floating_placement()
        .intersection_area(&workspace_rect)
        == 0
    } else {
      true
    };

    if should_recenter {
      window.set_floating_placement(
        window
          .floating_placement()
          .translate_to_center(&workspace.to_rect()?),
      );
    }
  }

  // Redraw full container tree.
  state
    .pending_sync
    .queue_container_to_redraw(state.root_container.clone());

  Ok(())
}

/// Finds the monitor matching the given display properties.
///
/// Returns the monitor and its index within the list of monitors.
///
/// # Platform-specific
///
/// - **macOS**: Matched on the display's UUID, which is stable.
/// - **Windows**: Matched in passes, strongest identifier first: device
///   path, then hardware ID (only when unique), then handle. Checking all
///   three against each monitor in turn would let a monitor early in the
///   list claim a display on a recycled handle before the monitor that
///   actually owns that panel could claim it by path.
fn find_matching_monitor<'a>(
  monitors: &'a [Monitor],
  properties: &NativeMonitorProperties,
) -> Option<(&'a Monitor, usize)> {
  #[cfg(target_os = "macos")]
  {
    monitors
      .iter()
      .enumerate()
      .find(|(_, monitor)| {
        monitor.native_properties().device_uuid == properties.device_uuid
      })
      .map(|(index, monitor)| (monitor, index))
  }

  #[cfg(target_os = "windows")]
  {
    let by_device_path = || {
      properties.device_path.as_deref().and_then(|device_path| {
        monitors.iter().enumerate().find(|(_, monitor)| {
          monitor.native_properties().device_path.as_deref()
            == Some(device_path)
        })
      })
    };

    // A hardware ID identifies a model, not a panel, so two identical
    // displays share one. It only tells monitors apart when unique on
    // both sides of the comparison.
    let by_hardware_id = || {
      properties.hardware_id.as_deref().and_then(|hardware_id| {
        let mut matched =
          monitors.iter().enumerate().filter(|(_, monitor)| {
            monitor.native_properties().hardware_id.as_deref()
              == Some(hardware_id)
          });

        match (matched.next(), matched.next()) {
          (Some(only), None) => Some(only),
          _ => None,
        }
      })
    };

    // Windows recycles handles: across a display wake, a handle that
    // belonged to one panel can be reissued to another. Last resort only.
    let by_handle = || {
      monitors.iter().enumerate().find(|(_, monitor)| {
        monitor.native_properties().handle == properties.handle
      })
    };

    by_device_path()
      .or_else(by_hardware_id)
      .or_else(by_handle)
      .map(|(index, monitor)| (monitor, index))
  }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
  use super::find_matching_monitor;
  use crate::models::{Monitor, NativeMonitorProperties};

  /// Builds a monitor carrying only the identifiers that matching uses.
  fn monitor(
    handle: isize,
    device_path: Option<&str>,
    hardware_id: Option<&str>,
  ) -> Monitor {
    Monitor::mock()
      .handle(handle)
      .maybe_device_path(device_path.map(ToString::to_string))
      .maybe_hardware_id(hardware_id.map(ToString::to_string))
      .call()
  }

  /// Builds the properties of an incoming display.
  fn display(
    handle: isize,
    device_path: Option<&str>,
    hardware_id: Option<&str>,
  ) -> NativeMonitorProperties {
    NativeMonitorProperties::mock()
      .handle(handle)
      .maybe_device_path(device_path.map(ToString::to_string))
      .maybe_hardware_id(hardware_id.map(ToString::to_string))
      .call()
  }

  /// Taken from a real display wake, where Windows reissued handle
  /// `131277` from the WAC1014 panel to the ENC2775 one.
  ///
  /// The recycled handle belongs to the monitor listed *first*, so a
  /// per-monitor check that tried handles before paths would hand ENC2775
  /// the wrong monitor and move its workspaces to another screen.
  #[test]
  fn device_path_wins_over_a_recycled_handle() {
    let monitors = vec![
      monitor(131_277, Some(r"\\?\DISPLAY#WAC1014"), Some("WAC1014")),
      monitor(30_542_173, Some(r"\\?\DISPLAY#TSB2017"), Some("TSB2017")),
      monitor(
        1_532_756_959,
        Some(r"\\?\DISPLAY#ENC2775"),
        Some("ENC2775"),
      ),
    ];

    let incoming =
      display(131_277, Some(r"\\?\DISPLAY#ENC2775"), Some("ENC2775"));

    let (matched, index) = find_matching_monitor(&monitors, &incoming)
      .expect("the ENC2775 monitor should match");

    assert_eq!(index, 2);
    assert_eq!(
      matched.native_properties().device_path.as_deref(),
      Some(r"\\?\DISPLAY#ENC2775")
    );
  }

  /// Two panels of the same model share a hardware ID, so it identifies
  /// neither of them.
  #[test]
  fn a_shared_hardware_id_does_not_match() {
    let monitors = vec![
      monitor(1, None, Some("ENC2775")),
      monitor(2, None, Some("ENC2775")),
    ];

    let incoming = display(99, None, Some("ENC2775"));

    assert!(find_matching_monitor(&monitors, &incoming).is_none());
  }

  /// The handle is still the fallback when nothing stronger is available.
  #[test]
  fn handle_matches_when_no_path_or_hardware_id() {
    let monitors = vec![monitor(1, None, None), monitor(2, None, None)];
    let incoming = display(2, None, None);

    let (_, index) = find_matching_monitor(&monitors, &incoming)
      .expect("the second monitor should match by handle");

    assert_eq!(index, 1);
  }

  /// A display the WM has never seen must not be forced onto an existing
  /// monitor.
  #[test]
  fn an_unknown_display_matches_nothing() {
    let monitors =
      vec![monitor(1, Some(r"\\?\DISPLAY#TSB2017"), Some("TSB2017"))];

    let incoming =
      display(42, Some(r"\\?\DISPLAY#NEW0001"), Some("NEW0001"));

    assert!(find_matching_monitor(&monitors, &incoming).is_none());
  }
}
