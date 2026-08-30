use tracing::info;

use super::move_workspace_to_monitor;
use crate::{
  traits::CommonGetters, user_config::UserConfig, wm_state::WmState,
};

/// Moves displaced workspaces back to the monitor they came from.
///
/// Tearing a monitor down moves its workspaces onto a surviving monitor so
/// their windows aren't lost, and stamps each one with the monitor it
/// left. This puts them back once that monitor reappears, which is what
/// stops a spurious disconnect (e.g. a display wake that reissues monitor
/// handles) from permanently piling every workspace onto one screen.
///
/// Workspaces with an explicit monitor binding are left alone, since their
/// config already decides where they belong.
///
/// Should only be called once the monitors have been sorted, since a
/// workspace bound by index is matched against the monitor's position.
pub fn restore_workspaces_to_origin(
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  for workspace in state.workspaces() {
    let Some(origin_id) = workspace.origin_monitor_id() else {
      continue;
    };

    // An explicit binding outranks where the workspace happened to be, and
    // has already been honoured by
    // `move_bounded_workspaces_to_new_monitor`.
    if workspace.config().is_bound() {
      workspace.set_origin_monitor_id(None);
      continue;
    }

    let origin_monitor = state
      .monitors()
      .into_iter()
      .find(|monitor| monitor.stable_id().as_deref() == Some(&origin_id));

    // Leave the marker in place while the monitor is still missing, so the
    // workspace goes home whenever it does come back.
    let Some(origin_monitor) = origin_monitor else {
      continue;
    };

    workspace.set_origin_monitor_id(None);

    if workspace
      .monitor()
      .is_some_and(|monitor| monitor.id() == origin_monitor.id())
    {
      continue;
    }

    info!("Restoring workspace to its original monitor: {workspace}");

    move_workspace_to_monitor(&workspace, &origin_monitor, state, config)?;
  }

  Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
  use tokio::sync::mpsc;
  use wm_common::WorkspaceConfig;
  use wm_platform::Dispatcher;

  use super::restore_workspaces_to_origin;
  use crate::{
    commands::{container::attach_container, monitor::remove_monitor},
    models::{Monitor, TilingWindow, Workspace},
    traits::CommonGetters,
    user_config::UserConfig,
    wm_state::WmState,
  };

  const PANEL_A: &str = r"\\?\DISPLAY#WAC1014";
  const PANEL_B: &str = r"\\?\DISPLAY#ENC2775";
  const PANEL_C: &str = r"\\?\DISPLAY#TSB2017";

  /// Builds a monitor carrying one workspace with a single window.
  ///
  /// The window matters: `remove_monitor` only moves workspaces that have
  /// children or are `keep_alive`.
  fn monitor(device_path: &str, workspace_name: &str) -> Monitor {
    let workspace = Workspace::mock()
      .name(workspace_name.to_string())
      .tiling_containers(vec![TilingWindow::mock().call().into()])
      .call();

    Monitor::mock()
      .device_path(device_path.to_string())
      .workspaces(vec![workspace])
      .call()
  }

  /// Builds a state with a monitor per `(panel, workspace name)` pair.
  fn fixture(monitors: &[(&str, &str)]) -> WmState {
    // A fresh state hasn't initialized, so `emit_event` is a no-op and the
    // receivers going out of scope doesn't matter.
    let (event_tx, _) = mpsc::unbounded_channel();
    let (exit_tx, _) = mpsc::unbounded_channel();

    let state = WmState::new(Dispatcher::mock(), event_tx, exit_tx);

    for (device_path, workspace_name) in monitors {
      attach(&state, monitor(device_path, workspace_name));
    }

    state
  }

  /// Attaches a monitor to the root container.
  fn attach(state: &WmState, monitor: Monitor) {
    attach_container(
      &monitor.into(),
      &state.root_container.clone().into(),
      None,
    )
    .unwrap();
  }

  fn workspace_by_name(state: &WmState, name: &str) -> Workspace {
    state
      .workspaces()
      .into_iter()
      .find(|workspace| workspace.config().name == name)
      .expect("workspace should exist")
  }

  /// The panel a workspace currently sits on.
  fn panel_of(state: &WmState, name: &str) -> Option<String> {
    workspace_by_name(state, name)
      .monitor()
      .and_then(|monitor| monitor.stable_id())
  }

  fn monitor_by_panel(state: &WmState, panel: &str) -> Monitor {
    state
      .monitors()
      .into_iter()
      .find(|monitor| monitor.stable_id().as_deref() == Some(panel))
      .expect("monitor should exist")
  }

  #[test]
  fn a_displaced_workspace_returns_when_its_monitor_comes_back() {
    // The bug this guards: only bound workspaces were moved back, so the
    // rest stayed piled on whichever monitor survived the disconnect.
    let mut state = fixture(&[(PANEL_A, "1"), (PANEL_B, "2")]);
    let config = UserConfig::mock(vec![]);

    remove_monitor(monitor_by_panel(&state, PANEL_B), &mut state, &config)
      .unwrap();

    // Workspace 2 is now stranded on monitor A.
    assert_eq!(panel_of(&state, "2").as_deref(), Some(PANEL_A));

    attach(&state, monitor(PANEL_B, "3"));
    restore_workspaces_to_origin(&mut state, &config).unwrap();

    assert_eq!(panel_of(&state, "2").as_deref(), Some(PANEL_B));
    assert!(workspace_by_name(&state, "2").origin_monitor_id().is_none());
  }

  #[test]
  fn a_displaced_workspace_waits_while_its_monitor_is_absent() {
    let mut state = fixture(&[(PANEL_A, "1"), (PANEL_B, "2")]);
    let config = UserConfig::mock(vec![]);

    remove_monitor(monitor_by_panel(&state, PANEL_B), &mut state, &config)
      .unwrap();
    restore_workspaces_to_origin(&mut state, &config).unwrap();

    // Still on monitor A, and still remembers where it belongs.
    assert_eq!(panel_of(&state, "2").as_deref(), Some(PANEL_A));
    assert_eq!(
      workspace_by_name(&state, "2")
        .origin_monitor_id()
        .as_deref(),
      Some(PANEL_B)
    );
  }

  #[test]
  fn a_bound_workspace_is_left_where_its_config_puts_it() {
    // An explicit binding decides where the workspace belongs, so the
    // origin marker is dropped rather than acted on.
    let mut state = fixture(&[(PANEL_A, "1"), (PANEL_B, "2")]);
    let config = UserConfig::mock(vec![]);

    let workspace = workspace_by_name(&state, "2");
    workspace.set_config(WorkspaceConfig {
      bind_to_monitor_id: Some(PANEL_A.to_string()),
      ..workspace.config()
    });

    remove_monitor(monitor_by_panel(&state, PANEL_B), &mut state, &config)
      .unwrap();

    attach(&state, monitor(PANEL_B, "3"));
    restore_workspaces_to_origin(&mut state, &config).unwrap();

    assert_eq!(panel_of(&state, "2").as_deref(), Some(PANEL_A));
    assert!(workspace_by_name(&state, "2").origin_monitor_id().is_none());
  }

  #[test]
  fn only_the_first_displacement_is_remembered() {
    // A workspace pushed across two monitors in a row still belongs to the
    // one it started on.
    let mut state =
      fixture(&[(PANEL_A, "1"), (PANEL_B, "2"), (PANEL_C, "3")]);
    let config = UserConfig::mock(vec![]);

    remove_monitor(monitor_by_panel(&state, PANEL_C), &mut state, &config)
      .unwrap();
    remove_monitor(monitor_by_panel(&state, PANEL_B), &mut state, &config)
      .unwrap();

    assert_eq!(
      workspace_by_name(&state, "3")
        .origin_monitor_id()
        .as_deref(),
      Some(PANEL_C)
    );
  }
}
