use anyhow::Context;
use tracing::info;

use super::{activate_workspace, focus_workspace};
use crate::{
  models::{Container, WorkspaceTarget},
  traits::CommonGetters,
  user_config::UserConfig,
  wm_state::WmState,
  workspace_store::{self, StoredWorkspace},
};

/// Adds a workspace that persists across restarts.
///
/// The workspace is bound to the monitor of the subject container, written
/// to the workspace store, then activated and focused.
///
/// If no name is given, the lowest unused positive integer is used, so the
/// name lines up with the numeric workspace keybindings.
pub fn add_workspace(
  name: Option<&str>,
  subject_container: &Container,
  state: &mut WmState,
  config: &mut UserConfig,
) -> anyhow::Result<()> {
  let monitor = subject_container
    .monitor()
    .context("No monitor to add the workspace to.")?;

  let name = match name {
    Some(name) => name.to_string(),
    None => next_workspace_name(state, config),
  };

  if state.workspace_by_name(&name).is_some() {
    anyhow::bail!("Workspace '{name}' is already active.");
  }

  let bind_to_monitor = u32::try_from(monitor.index())
    .context("Monitor index out of range.")?;

  let stored = StoredWorkspace {
    name: name.clone(),
    bind_to_monitor: Some(bind_to_monitor),
  };

  workspace_store::add(&config.path, stored.clone())?;

  // Mirror the store into the in-memory config, so the workspace behaves
  // like a declared one for the rest of the session without a reload.
  workspace_store::merge_into(&mut config.value.workspaces, &[stored]);

  info!("Adding workspace '{name}' bound to monitor {bind_to_monitor}.");

  activate_workspace(Some(&name), Some(monitor), state, config)?;

  focus_workspace(WorkspaceTarget::Name(name), state, config)
}

/// Lowest unused positive integer across the config and active workspaces.
fn next_workspace_name(state: &WmState, config: &UserConfig) -> String {
  let used = config
    .value
    .workspaces
    .iter()
    .map(|workspace| workspace.name.clone())
    .chain(
      state
        .workspaces()
        .iter()
        .map(|workspace| workspace.config().name),
    )
    .collect::<Vec<_>>();

  next_unused_name(&used)
}

/// Lowest positive integer not present in the given names.
///
/// Searching `1..=len + 1` is enough: `len` names can cover at most `len`
/// of those `len + 1` candidates, so one of them is always free.
fn next_unused_name(used: &[String]) -> String {
  let limit = u32::try_from(used.len())
    .unwrap_or(u32::MAX)
    .saturating_add(1);

  (1..=limit)
    .map(|index| index.to_string())
    .find(|name| !used.contains(name))
    .unwrap_or_else(|| "1".to_string())
}

#[cfg(test)]
mod tests {
  use super::next_unused_name;

  fn next(used: &[&str]) -> String {
    let used = used
      .iter()
      .map(|name| (*name).to_string())
      .collect::<Vec<_>>();

    next_unused_name(&used)
  }

  #[test]
  fn picks_the_next_number_after_a_full_range() {
    let used = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

    assert_eq!(next(&used), "10");
  }

  #[test]
  fn fills_a_gap_rather_than_appending() {
    assert_eq!(next(&["1", "2", "4"]), "3");
  }

  #[test]
  fn ignores_non_numeric_names() {
    assert_eq!(next(&["web", "mail"]), "1");
  }

  #[test]
  fn starts_at_one_when_nothing_is_used() {
    assert_eq!(next(&[]), "1");
  }
}
