use std::{
  fs::{self},
  path::PathBuf,
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::{path::BaseDirectory, AppHandle, Manager};

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

    // Install the starter widget pack if this is the first run.
    if installer.app_settings.is_first_run {
      installer.install_starter_pack()?;
    }

    Ok(Arc::new(installer))
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
  /// Two layouts have to work. In a bundled app the resources sit above
  /// the binary, which is what the first candidate covers. In a local
  /// build the binary lives in the workspace's shared `target/` directory,
  /// one level further out than it would in a standalone checkout, so the
  /// pack is under `bar/`.
  fn starter_pack_dir(&self) -> anyhow::Result<PathBuf> {
    let candidates =
      ["../../resources/starter", "../../bar/resources/starter"];

    for candidate in candidates {
      let path = self
        .app_handle
        .path()
        .resolve(candidate, BaseDirectory::Resource)
        .context("Unable to resolve starter pack resource.")?;

      if path.join("zpack.json").exists() {
        return Ok(path);
      }
    }

    anyhow::bail!(
      "Unable to locate the starter pack. Tried: {}.",
      candidates.join(", ")
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
