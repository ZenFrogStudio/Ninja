use std::ops::Not;

use anyhow::Context;
use tracing::info;
use wm_common::{TilingDirection, WmEvent, WorkspaceConfig};

use super::sort_workspaces;
use crate::{
  commands::container::attach_container,
  models::{Monitor, Workspace},
  traits::{CommonGetters, PositionGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

/// Activates a workspace on the target monitor.
///
/// If no workspace name is provided, the first suitable workspace defined
/// in the user's config will be used.
///
/// If no target monitor is provided, the workspace is activated on
/// whichever monitor it is bound to, or the currently focused monitor.
pub fn activate_workspace(
  workspace_name: Option<&str>,
  target_monitor: Option<Monitor>,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let workspace_config = workspace_config(
    workspace_name,
    target_monitor.clone(),
    state,
    config,
  )?;

  let target_monitor = target_monitor
    .or_else(|| {
      workspace_config
        .bind_to_monitor
        .and_then(|index| {
          state
            .monitors()
            .into_iter()
            .find(|monitor| monitor.index() == index as usize)
        })
        .or_else(|| {
          state
            .focused_container()
            .and_then(|focused| focused.monitor())
        })
    })
    .context("Failed to get a target monitor for the workspace.")?;

  let monitor_rect = target_monitor.to_rect()?;

  let tiling_direction = if monitor_rect.height() > monitor_rect.width() {
    TilingDirection::Vertical
  } else {
    TilingDirection::Horizontal
  };

  let workspace = Workspace::new(
    workspace_config.clone(),
    config.value.gaps.clone(),
    tiling_direction,
  );

  // Attach the created workspace to the specified monitor.
  attach_container(
    &workspace.clone().into(),
    &target_monitor.clone().into(),
    None,
  )?;

  sort_workspaces(&target_monitor, config)?;

  info!("Activating workspace: {workspace}");

  state.emit_event(WmEvent::WorkspaceActivated {
    activated_workspace: workspace.to_dto()?,
  });

  Ok(())
}

/// Gets config for the workspace to activate.
fn workspace_config(
  workspace_name: Option<&str>,
  target_monitor: Option<Monitor>,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WorkspaceConfig> {
  let found_config = match workspace_name {
    Some(workspace_name) => config
      .inactive_workspace_configs(&state.workspaces())
      .into_iter()
      .find(|config| config.name == workspace_name)
      .cloned()
      // Workspaces don't have to be declared in the config. An unknown
      // name creates one on demand, matching how i3 and sway behave.
      // These aren't `keep_alive`, so they're torn down once empty.
      .or_else(|| {
        state
          .workspaces()
          .iter()
          .any(|workspace| workspace.config().name == workspace_name)
          .not()
          .then(|| WorkspaceConfig {
            name: workspace_name.to_string(),
            display_name: None,
            bind_to_monitor: None,
            keep_alive: false,
          })
      })
      .with_context(|| {
        format!("Workspace '{workspace_name}' is already active.")
      }),
    None => target_monitor
      .and_then(|target_monitor| {
        config.workspace_config_for_monitor(
          &target_monitor,
          &state.workspaces(),
        )
      })
      .or_else(|| {
        config.next_inactive_workspace_config(&state.workspaces())
      })
      .cloned()
      .context("No workspace config available to activate workspace."),
  };

  found_config
}
