use std::{
  fs::{self},
  path::{Component, Path, PathBuf},
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{
  app_settings::AppSettings,
  common::{copy_dir_all, read_and_parse_json},
  widget_pack::WidgetPackConfig,
};

/// The ID of the built-in starter pack.
pub const STARTER_PACK_ID: &str = "ninja.starter";

/// Metadata about an installed widget pack.
///
/// These are stored in `%userprofile%/.ninja/bar/.marketplace`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackMetadata {
  /// Unique identifier for the pack.
  pub pack_id: String,

  /// Version of the installed pack.
  pub version: String,

  /// Installation timestamp, stored as seconds since epoch.
  pub installed_at: u64,
}

impl PackMetadata {
  pub fn new(pack_id: &str, version: &str) -> anyhow::Result<Self> {
    Ok(Self {
      pack_id: pack_id.to_string(),
      version: version.to_string(),
      installed_at: SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("Failed to get timestamp.")?
        .as_secs(),
    })
  }
}

/// Manages installation of bundled widget packs.
///
/// Packs are only ever installed from resources shipped with the
/// application. There is no remote install path.
#[derive(Debug)]
pub struct PackInstaller {
  /// Handle to the Tauri application.
  app_handle: AppHandle,

  /// Reference to `AppSettings`.
  app_settings: Arc<AppSettings>,
}

impl PackInstaller {
  /// Creates a new `PackInstaller` instance.
  pub fn new(
    app_handle: &AppHandle,
    app_settings: Arc<AppSettings>,
  ) -> anyhow::Result<Arc<Self>> {
    let installer = Self {
      app_handle: app_handle.clone(),
      app_settings,
    };

    // Install the starter widget pack on first run, and again whenever
    // its files have gone missing. A config directory outlives the
    // downloaded packs — migrating from an install that stored them under
    // a different application name leaves the config in place but the
    // packs unreachable — and without this the bar starts with no widgets.
    if installer.app_settings.is_first_run
      || !installer.is_starter_pack_installed()
    {
      installer.install_starter_pack()?;
    }

    Ok(Arc::new(installer))
  }

  /// Whether the starter pack's files are present on disk.
  ///
  /// Metadata alone isn't enough to answer this: it records that a pack
  /// was installed, not that its files are still where they were put.
  fn is_starter_pack_installed(&self) -> bool {
    let metadata_path = self
      .app_settings
      .marketplace_pack_metadata_path(STARTER_PACK_ID);

    let Ok(metadata) = read_and_parse_json::<PackMetadata>(&metadata_path)
    else {
      return false;
    };

    self
      .app_settings
      .marketplace_pack_download_dir(STARTER_PACK_ID, &metadata.version)
      .join("zpack.json")
      .exists()
  }

  /// Returns a vector of `PackMetadata` instances for all installed packs.
  pub fn installed_packs_metadata(
    &self,
  ) -> anyhow::Result<Vec<PackMetadata>> {
    let packs_metadata =
      fs::read_dir(&self.app_settings.marketplace_meta_dir)?
        .filter_map(|entry| {
          let metadata =
            read_and_parse_json::<PackMetadata>(&entry.ok()?.path())
              .ok()?;

          Some(metadata)
        })
        .collect();

    Ok(packs_metadata)
  }

  /// Deletes the metadata for an installed widget pack.
  pub fn delete_metadata(&self, pack_id: &str) -> anyhow::Result<()> {
    let metadata_path =
      self.app_settings.marketplace_pack_metadata_path(pack_id);

    if metadata_path.exists() {
      fs::remove_file(metadata_path)?;
    }

    Ok(())
  }

  /// Locates the bundled `starter` pack resource.
  ///
  /// Two layouts have to work. When bundled, Tauri rewrites the leading
  /// `..` of a resource glob into an `_up_` directory, so the pack lands
  /// beneath the resource directory rather than above it. In a local build
  /// the binary sits in the workspace's shared `target/` directory, one
  /// level further out than a standalone checkout, so the pack is reached
  /// by walking up to the repository root.
  fn starter_pack_dir(&self) -> anyhow::Result<PathBuf> {
    let resource_dir = self
      .app_handle
      .path()
      .resource_dir()
      .context("Unable to resolve resource directory.")?;

    let candidates = [
      // Bundled.
      "_up_/_up_/resources/starter",
      "resources/starter",
      // Local build.
      "../../bar/resources/starter",
      "../../resources/starter",
    ];

    let mut tried = Vec::new();

    for candidate in candidates {
      let path = normalize_path(&resource_dir.join(candidate));

      if path.join("zpack.json").exists() {
        return Ok(path);
      }

      tried.push(path.display().to_string());
    }

    anyhow::bail!(
      "Unable to locate the starter pack. Tried: {}.",
      tried.join(", ")
    )
  }

  /// Installs the starter widget pack from the embedded
  /// `starter` resource.
  fn install_starter_pack(&self) -> anyhow::Result<()> {
    let starter_pack_dir = self.starter_pack_dir()?;

    let pack_config = read_and_parse_json::<WidgetPackConfig>(
      &starter_pack_dir.join("zpack.json"),
    )?;

    let dest_dir = self.app_settings.marketplace_pack_download_dir(
      STARTER_PACK_ID,
      &pack_config.version,
    );

    // Copy the starter pack files.
    fs::create_dir_all(&dest_dir)?;
    copy_dir_all(&starter_pack_dir, &dest_dir, true)?;

    let metadata =
      PackMetadata::new(STARTER_PACK_ID, &pack_config.version)?;

    // Write metadata file.
    fs::write(
      self
        .app_settings
        .marketplace_pack_metadata_path(STARTER_PACK_ID),
      serde_json::to_string_pretty(&metadata)? + "\n",
    )?;

    Ok(())
  }
}

/// Collapses `.` and `..` components without touching the filesystem.
///
/// Windows leaves `..` alone inside a verbatim (`\\?\`) path, which is
/// what the resource directory can be, so a path built by joining `..`
/// onto it never matches anything on disk. Resolving the components up
/// front avoids that, and unlike `canonicalize` it works for a path that
/// doesn't exist.
fn normalize_path(path: &Path) -> PathBuf {
  let mut normalized = PathBuf::new();

  for component in path.components() {
    match component {
      Component::ParentDir => {
        normalized.pop();
      }
      Component::CurDir => {}
      component => normalized.push(component),
    }
  }

  normalized
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use super::normalize_path;

  #[test]
  fn collapses_parent_components() {
    assert_eq!(
      normalize_path(Path::new("/a/b/c/../../d")),
      PathBuf::from("/a/d")
    );
  }

  #[test]
  fn drops_current_dir_components() {
    assert_eq!(
      normalize_path(Path::new("/a/./b/./c")),
      PathBuf::from("/a/b/c")
    );
  }

  #[test]
  fn leaves_a_plain_path_alone() {
    assert_eq!(
      normalize_path(Path::new("/a/b/c")),
      PathBuf::from("/a/b/c")
    );
  }
}
