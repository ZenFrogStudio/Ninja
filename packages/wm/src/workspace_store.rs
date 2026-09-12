use std::{
  fs,
  path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use wm_common::WorkspaceConfig;

/// Name of the store file, kept alongside the user's config file.
const STORE_FILE_NAME: &str = "workspaces.json";

/// A workspace added at runtime (e.g. via the bar's add button).
///
/// These are held outside the user's config file so that the WM never has
/// to rewrite it, which would discard the user's comments and formatting.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredWorkspace {
  /// Name of the workspace.
  pub name: String,

  /// Index of the monitor the workspace belongs to.
  ///
  /// Only written by versions that predate `bind_to_monitor_id`, and kept
  /// as a fallback so their stores keep working. A monitor index changes
  /// whenever the displays are rearranged, so it can't identify a panel
  /// on its own.
  #[serde(default)]
  pub bind_to_monitor: Option<u32>,

  /// Stable identifier of the monitor the workspace belongs to.
  ///
  /// Required for the workspace to reappear on the right screen, since
  /// only bound workspaces are activated when a monitor is attached.
  /// `None` when the platform couldn't identify the panel, in which
  /// case `bind_to_monitor` carries the binding instead.
  #[serde(default)]
  pub bind_to_monitor_id: Option<String>,
}

impl StoredWorkspace {
  /// Converts this entry into a `WorkspaceConfig`.
  ///
  /// Stored workspaces are always `keep_alive`. They were added
  /// deliberately, so they shouldn't be torn down once they're empty.
  fn to_config(&self) -> WorkspaceConfig {
    WorkspaceConfig {
      name: self.name.clone(),
      display_name: None,
      bind_to_monitor: self.bind_to_monitor,
      bind_to_monitor_id: self.bind_to_monitor_id.clone(),
      keep_alive: true,
    }
  }
}

/// Path of the store file for a given config path.
///
/// The store sits next to the config so that a config passed via
/// `--config` keeps its own set of added workspaces.
pub fn store_path(config_path: &Path) -> PathBuf {
  config_path.with_file_name(STORE_FILE_NAME)
}

/// Reads the stored workspaces for a given config path.
///
/// A missing or malformed store is treated as empty, since added
/// workspaces are a convenience and shouldn't prevent startup. A malformed
/// store is logged as a warning.
pub fn read(config_path: &Path) -> Vec<StoredWorkspace> {
  let path = store_path(config_path);

  if !path.exists() {
    return Vec::new();
  }

  match fs::read_to_string(&path)
    .map_err(anyhow::Error::from)
    .and_then(|contents| parse(&contents))
  {
    Ok(workspaces) => workspaces,
    Err(err) => {
      tracing::warn!(
        "Ignoring workspace store at {}: {err:?}",
        path.display()
      );

      Vec::new()
    }
  }
}

/// Adds a workspace to the store, replacing any entry with the same name.
///
/// The file is written to a temporary path and renamed over the original,
/// so an interrupted write can't leave a half-written store behind.
pub fn add(
  config_path: &Path,
  workspace: StoredWorkspace,
) -> anyhow::Result<()> {
  let path = store_path(config_path);

  let mut workspaces = read(config_path);
  workspaces.retain(|entry| entry.name != workspace.name);
  workspaces.push(workspace);

  let parent_dir = path.parent().context("Invalid store path.")?;
  fs::create_dir_all(parent_dir)?;

  let temp_path = path.with_extension("json.tmp");
  fs::write(&temp_path, serialize(&workspaces)?)?;
  fs::rename(&temp_path, &path)?;

  Ok(())
}

/// Appends the stored workspaces to a config's workspace list.
///
/// Entries whose name is already declared in the config are skipped, so
/// the user's config always wins over the store.
pub fn merge_into(
  workspaces: &mut Vec<WorkspaceConfig>,
  stored: &[StoredWorkspace],
) {
  for entry in stored {
    let is_declared = workspaces
      .iter()
      .any(|workspace| workspace.name == entry.name);

    if !is_declared {
      workspaces.push(entry.to_config());
    }
  }
}

/// Parses the contents of a store file.
fn parse(contents: &str) -> anyhow::Result<Vec<StoredWorkspace>> {
  // An empty file is a valid empty store.
  if contents.trim().is_empty() {
    return Ok(Vec::new());
  }

  serde_json::from_str(contents).context("Invalid workspace store.")
}

/// Serializes the store file contents.
fn serialize(workspaces: &[StoredWorkspace]) -> anyhow::Result<String> {
  let mut contents = serde_json::to_string_pretty(workspaces)?;
  contents.push('\n');

  Ok(contents)
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use wm_common::WorkspaceConfig;

  use super::{merge_into, parse, serialize, store_path, StoredWorkspace};

  fn stored(name: &str, monitor: Option<u32>) -> StoredWorkspace {
    StoredWorkspace {
      name: name.to_string(),
      bind_to_monitor: monitor,
      bind_to_monitor_id: None,
    }
  }

  /// A store entry bound to a panel rather than a monitor index.
  fn stored_by_id(name: &str, monitor_id: &str) -> StoredWorkspace {
    StoredWorkspace {
      name: name.to_string(),
      bind_to_monitor: None,
      bind_to_monitor_id: Some(monitor_id.to_string()),
    }
  }

  fn declared(name: &str) -> WorkspaceConfig {
    WorkspaceConfig {
      name: name.to_string(),
      display_name: None,
      bind_to_monitor: Some(0),
      bind_to_monitor_id: None,
      keep_alive: true,
    }
  }

  #[test]
  fn store_sits_next_to_the_config() {
    let path = store_path(Path::new("/cfg/config.yaml"));

    assert_eq!(path, Path::new("/cfg/workspaces.json"));
  }

  #[test]
  fn parses_camel_case_entries() {
    let parsed = parse(r#"[{"name":"10","bindToMonitor":1}]"#).unwrap();

    assert_eq!(parsed, vec![stored("10", Some(1))]);
  }

  #[test]
  fn treats_an_empty_file_as_an_empty_store() {
    assert_eq!(parse("").unwrap(), Vec::new());
    assert_eq!(parse("  \n").unwrap(), Vec::new());
  }

  #[test]
  fn rejects_malformed_contents() {
    assert!(parse("not json").is_err());
  }

  #[test]
  fn round_trips_through_serialization() {
    let workspaces = vec![
      stored("10", Some(1)),
      stored("11", None),
      stored_by_id("12", r"\\?\DISPLAY#ENC2775"),
    ];
    let parsed = parse(&serialize(&workspaces).unwrap()).unwrap();

    assert_eq!(parsed, workspaces);
  }

  #[test]
  fn parses_a_store_written_before_monitor_ids() {
    // Stores written by earlier versions carry only an index, and have to
    // keep binding by it.
    let parsed = parse(r#"[{"name":"5","bindToMonitor":1}]"#).unwrap();

    assert_eq!(parsed, vec![stored("5", Some(1))]);
    assert_eq!(parsed[0].to_config().bind_to_monitor, Some(1));
  }

  #[test]
  fn a_monitor_id_survives_the_conversion_to_config() {
    let config = stored_by_id("5", r"\\?\DISPLAY#ENC2775").to_config();

    assert_eq!(
      config.bind_to_monitor_id.as_deref(),
      Some(r"\\?\DISPLAY#ENC2775")
    );
    assert_eq!(config.bind_to_monitor, None);
    assert!(config.is_bound());
  }

  #[test]
  fn merges_stored_workspaces_as_keep_alive() {
    let mut workspaces = vec![declared("1")];
    merge_into(&mut workspaces, &[stored("10", Some(2))]);

    assert_eq!(workspaces.len(), 2);
    assert_eq!(workspaces[1].name, "10");
    assert_eq!(workspaces[1].bind_to_monitor, Some(2));
    assert!(workspaces[1].keep_alive);
  }

  #[test]
  fn config_wins_over_the_store() {
    let mut workspaces = vec![declared("1")];
    merge_into(&mut workspaces, &[stored("1", Some(2))]);

    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].bind_to_monitor, Some(0));
  }
}
