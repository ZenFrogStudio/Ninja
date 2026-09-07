mod config;

pub use config::*;

use std::{
  collections::HashMap,
  fs::{self},
  path::Path,
  sync::Arc,
};

use anyhow::Context;
use tokio::sync::{broadcast, Mutex};

use crate::{
  app_settings::{AppSettings, VERSION_NUMBER},
  common::{read_and_parse_json, PathExt},
  pack_installer::{PackInstaller, PackMetadata},
};

#[derive(Debug)]
pub struct WidgetPackManager {
  /// Reference to `AppSettings`.
  app_settings: Arc<AppSettings>,

  /// Reference to `PackInstaller`.
  pack_installer: Arc<PackInstaller>,

  /// Map of widget packs by their ID's.
  pub widget_packs: Arc<Mutex<HashMap<String, WidgetPack>>>,

  _widget_packs_change_rx:
    broadcast::Receiver<HashMap<String, WidgetPack>>,

  pub widget_packs_change_tx:
    broadcast::Sender<HashMap<String, WidgetPack>>,

  _widget_configs_change_rx: broadcast::Receiver<(String, WidgetConfig)>,

  pub widget_configs_change_tx: broadcast::Sender<(String, WidgetConfig)>,
}

impl WidgetPackManager {
  /// Reads the pack config files within the config directory.
  ///
  /// Returns a new `WidgetPackManager` instance.
  pub fn new(
    app_settings: Arc<AppSettings>,
    pack_installer: Arc<PackInstaller>,
  ) -> anyhow::Result<Self> {
    let widget_packs =
      Self::read_widget_packs(&app_settings, &pack_installer)?;

    let (widget_packs_change_tx, _widget_packs_change_rx) =
      broadcast::channel(16);

    let (widget_configs_change_tx, _widget_configs_change_rx) =
      broadcast::channel(16);

    Ok(Self {
      app_settings,
      pack_installer,
      widget_packs: Arc::new(Mutex::new(widget_packs)),
      _widget_packs_change_rx,
      widget_packs_change_tx,
      _widget_configs_change_rx,
      widget_configs_change_tx,
    })
  }

  /// Reads all widget packs from:
  ///  - The user's config directory.
  ///  - The marketplace directory.
  ///
  /// Returns a hashmap of widget pack ID's to `WidgetPack` instances.
  fn read_widget_packs(
    app_settings: &AppSettings,
    pack_installer: &PackInstaller,
  ) -> anyhow::Result<HashMap<String, WidgetPack>> {
    let mut packs = HashMap::new();

    packs
      .extend(Self::read_custom_widget_packs(&app_settings.config_dir)?);

    for metadata in pack_installer.installed_packs_metadata()? {
      if let Ok(pack) = Self::read_widget_pack(
        &app_settings
          .marketplace_pack_download_dir(
            &metadata.pack_id,
            &metadata.version,
          )
          .join("zpack.json"),
        Some(&metadata),
      ) {
        packs.insert(metadata.pack_id.clone(), pack);
      } else {
        tracing::warn!(
          "Skipping marketplace pack at '{}' because it is invalid.",
          metadata.pack_id
        );
      }
    }

    Ok(packs)
  }

  /// Finds all valid widget packs within the user's config directory.
  ///
  /// Widget packs are at the 2nd-level of the config directory
  /// (i.e. `<CONFIG_DIR>/*/zpack.json`).
  ///
  /// Returns a hashmap of widget pack ID's to `WidgetPack` instances.
  fn read_custom_widget_packs(
    config_dir: &Path,
  ) -> anyhow::Result<HashMap<String, WidgetPack>> {
    // Get paths to the subdirectories within the config directory.
    let pack_dirs = fs::read_dir(config_dir)
      .with_context(|| {
        format!(
          "Failed to read config directory: {}",
          config_dir.display()
        )
      })?
      .filter_map(|entry| Some(entry.ok()?.path()))
      .filter(|path| path.is_dir());

    let mut packs = HashMap::new();

    // Parse the found config files.
    for pack_dir in pack_dirs {
      let pack_config_path = pack_dir.join("zpack.json");

      if !pack_config_path.exists() {
        warn!(
          "Skipping subdirectory at '{}' because it has no `zpack.json` file.",
          pack_dir.display()
        );

        continue;
      }

      match Self::read_widget_pack(&pack_config_path, None) {
        Ok(pack) => {
          tracing::info!(
            "Found valid widget pack at: {}",
            pack_dir.display()
          );

          packs.insert(pack.id.clone(), pack);
        }
        Err(err) => {
          error!("{:?}", err);
        }
      }
    }

    Ok(packs)
  }

  /// Reads a widget pack from a directory. Expects the pack config
  /// file (`zpack.json`) to be present.
  ///
  /// Returns a `WidgetPack` instance.
  pub fn read_widget_pack(
    config_path: &Path,
    metadata: Option<&PackMetadata>,
  ) -> anyhow::Result<WidgetPack> {
    let pack_config = read_and_parse_json::<WidgetPackConfig>(config_path)
      .map_err(|err| {
        anyhow::anyhow!(
          "Failed to parse widget pack at '{}': {:?}",
          config_path.display(),
          err
        )
      })?;

    let pack_dir = config_path.parent().with_context(|| {
      format!(
        "Invalid widget pack config path: {}.",
        config_path.display()
      )
    })?;

    let pack = WidgetPack {
      id: match metadata {
        Some(metadata) => metadata.pack_id.clone(),
        None => pack_config.name.to_string(),
      },
      r#type: match metadata {
        Some(_) => WidgetPackType::Marketplace,
        None => WidgetPackType::Custom,
      },
      config_path: config_path.canonicalize_pretty()?,
      directory_path: pack_dir.canonicalize_pretty()?,
      config: pack_config,
      metadata: metadata.cloned(),
    };

    Ok(pack)
  }

  /// Returns all widget packs as a hashmap.
  pub async fn widget_packs(&self) -> HashMap<String, WidgetPack> {
    self.widget_packs.lock().await.clone()
  }

  /// Finds a widget pack by ID.
  pub async fn widget_pack_by_id(
    &self,
    pack_id: &str,
  ) -> Option<WidgetPack> {
    let widget_packs = self.widget_packs.lock().await;
    widget_packs.get(pack_id).cloned()
  }

  /// Finds a custom widget pack by ID.
  ///
  /// Returns an error if the widget pack is not found or is not a
  /// custom pack.
  async fn find_custom_widget_pack(
    &self,
    pack_id: &str,
  ) -> anyhow::Result<WidgetPack> {
    self
      .widget_pack_by_id(pack_id)
      .await
      .filter(|pack| pack.r#type == WidgetPackType::Custom)
      .context(format!("Custom widget pack not found: {}", pack_id))
  }

  /// Updates the widget config for the given pack and widget name.
  pub async fn update_widget_config(
    &self,
    pack_id: &str,
    widget_name: &str,
    new_config: WidgetConfig,
  ) -> anyhow::Result<WidgetConfig> {
    tracing::info!("Updating widget config for {}.", widget_name);

    let pack = self.find_custom_widget_pack(pack_id).await?;

    let mut widgets = pack.config.widgets.clone();
    let widget_index = widgets
      .iter()
      .position(|w| w.name == widget_name)
      .context(format!("Widget config not found for {}.", widget_name))?;

    widgets[widget_index] = new_config.clone();

    // Update the pack config to persist changes to disk.
    self
      .update_widget_pack(
        pack_id,
        UpdateWidgetPackArgs {
          widgets: Some(widgets),
          ..Default::default()
        },
      )
      .await?;

    // Emit the changed config.
    self
      .widget_configs_change_tx
      .send((pack_id.to_string(), new_config.clone()))?;

    Ok(new_config)
  }

  /// Creates a new widget pack.
  pub async fn create_widget_pack(
    &self,
    args: CreateWidgetPackArgs,
  ) -> anyhow::Result<WidgetPack> {
    validate_pack_name(&args.name)?;

    let pack_dir = self.app_settings.config_dir.join(&args.name);
    ensure_direct_child(&pack_dir, &self.app_settings.config_dir)?;

    let mut context = tera::Context::new();
    context.insert("PACK_NAME", &args.name);
    context.insert("PACK_VERSION", &args.version);
    context.insert("PACK_DESCRIPTION", &args.description);
    context.insert("PACK_TAGS", &args.tags);
    context.insert("REPOSITORY_URL", &args.repository_url);
    context.insert("NINJA_VERSION", &VERSION_NUMBER.to_string());

    self.app_settings.init_template(
      Path::new("pack-template"),
      &pack_dir,
      &context,
    )?;

    // Initialize git repository. Ignore errors (in case Git is not
    // installed).
    let _ = std::process::Command::new("git")
      .arg("init")
      .current_dir(&pack_dir)
      .output();

    let pack = Self::read_widget_pack(&pack_dir.join("zpack.json"), None)?;

    // Add the new widget pack to state.
    {
      let mut widget_packs = self.widget_packs.lock().await;
      widget_packs.insert(pack.id.clone(), pack.clone());

      // Broadcast the change.
      let _ = self.widget_packs_change_tx.send(widget_packs.clone());
    }

    Ok(pack)
  }

  /// Updates a widget pack.
  pub async fn update_widget_pack(
    &self,
    pack_id: &str,
    args: UpdateWidgetPackArgs,
  ) -> anyhow::Result<WidgetPack> {
    let mut pack = self.find_custom_widget_pack(pack_id).await?;
    let pack_id = pack.id.clone();

    // Update pack config fields.
    pack.config.name = args.name.clone().unwrap_or(pack.config.name);
    pack.config.version = args.version.unwrap_or(pack.config.version);
    pack.config.description =
      args.description.unwrap_or(pack.config.description);
    pack.config.tags = args.tags.unwrap_or(pack.config.tags);
    pack.config.preview_images =
      args.preview_images.unwrap_or(pack.config.preview_images);
    pack.config.repository_url =
      args.repository_url.unwrap_or(pack.config.repository_url);
    pack.config.widgets = args.widgets.unwrap_or(pack.config.widgets);

    // Write the updated pack config to file.
    fs::write(
      &pack.config_path,
      serde_json::to_string_pretty(&pack.config)? + "\n",
    )?;

    let mut widget_packs = self.widget_packs.lock().await;

    // Update the pack ID and remove the old entry if a new name is
    // provided.
    if let Some(new_name) = args.name {
      pack.id = new_name;
      widget_packs.remove(&pack_id);
    }

    // Broadcast the change.
    widget_packs.insert(pack.id.clone(), pack.clone());
    let _ = self.widget_packs_change_tx.send(widget_packs.clone());

    Ok(pack)
  }

  /// Deletes a widget pack.
  ///
  /// Removes the pack directory and all its contents.
  pub async fn delete_widget_pack(
    &self,
    pack_id: &str,
  ) -> anyhow::Result<()> {
    let pack = self
      .widget_pack_by_id(pack_id)
      .await
      .with_context(|| format!("Widget pack not found: {}", pack_id))?;

    match pack.r#type {
      WidgetPackType::Custom => {
        // Remove the directory with all widget files.
        fs::remove_dir_all(&pack.directory_path)?;
      }
      WidgetPackType::Marketplace => {
        self.pack_installer.delete_metadata(pack_id)?;
      }
    }

    // Remove the pack from state.
    {
      let mut widget_packs = self.widget_packs.lock().await;
      widget_packs.remove(pack_id);

      // Broadcast the change.
      let _ = self.widget_packs_change_tx.send(widget_packs.clone());
    }

    // Remove startup configs for the removed pack.
    self
      .app_settings
      .remove_startup_config(pack_id, None, None)
      .await?;

    // TODO: Kill active widget instances from the removed pack.

    Ok(())
  }

  /// Creates a new widget from a template.
  ///
  /// Adds a new entry to the pack config and copies the appropriate
  /// frontend template (e.g. React, Solid) to the widget's sub-directory.
  pub async fn create_widget_config(
    &self,
    args: CreateWidgetConfigArgs,
  ) -> anyhow::Result<WidgetConfig> {
    validate_pack_name(&args.name)?;

    let pack = self.find_custom_widget_pack(&args.pack_id).await?;
    let widget_dir = pack.directory_path.join(&args.name);
    ensure_direct_child(&widget_dir, &pack.directory_path)?;

    let template_path = match args.template {
      FrontendTemplate::ReactBuildless => {
        "widget-templates/react-buildless"
      }
      FrontendTemplate::SolidTypescript => "widget-templates/solid-ts",
    };

    let mut context = tera::Context::new();
    context.insert("WIDGET_NAME", &args.name);
    context.insert("NINJA_VERSION", &VERSION_NUMBER.to_string());

    self.app_settings.init_template(
      Path::new(template_path),
      &widget_dir,
      &context,
    )?;

    let widget_config = WidgetConfig {
      name: args.name.clone(),
      html_path: match args.template {
        FrontendTemplate::ReactBuildless => {
          format!("{}/index.html", args.name).into()
        }
        FrontendTemplate::SolidTypescript => {
          format!("{}/dist/index.html", args.name).into()
        }
      },
      z_order: ZOrder::Normal,
      shown_in_taskbar: false,
      focused: false,
      resizable: false,
      transparent: false,
      include_files: match args.template {
        FrontendTemplate::ReactBuildless => {
          vec![format!("{}/**", args.name)]
        }
        FrontendTemplate::SolidTypescript => {
          vec![format!("{}/dist/**", args.name)]
        }
      },
      caching: WidgetCaching::default(),
      privileges: WidgetPrivileges::default(),
      presets: vec![WidgetPreset {
        name: "default".to_string(),
        placement: WidgetPlacement {
          anchor: AnchorPoint::TopLeft,
          offset_x: "0px".parse()?,
          offset_y: "0px".parse()?,
          width: "100%".parse()?,
          height: "40px".parse()?,
          monitor_selection: MonitorSelection::All,
          dock_to_edge: DockConfig::default(),
        },
      }],
    };

    // Add widget to pack config.
    let mut widgets = pack.config.widgets.clone();
    widgets.push(widget_config.clone());

    self
      .update_widget_pack(
        &args.pack_id,
        UpdateWidgetPackArgs {
          widgets: Some(widgets),
          ..Default::default()
        },
      )
      .await?;

    Ok(widget_config)
  }

  /// Deletes a widget from a pack.
  ///
  /// Removes the entry from the pack config and deletes the widget's
  /// sub-directory.
  pub async fn delete_widget_config(
    &self,
    pack_id: &str,
    widget_name: &str,
  ) -> anyhow::Result<()> {
    let pack = self.find_custom_widget_pack(pack_id).await?;

    // Remove widget from pack config.
    let mut widgets = pack.config.widgets.clone();
    widgets.retain(|widget| widget.name != widget_name);

    self
      .update_widget_pack(
        pack_id,
        UpdateWidgetPackArgs {
          widgets: Some(widgets),
          ..Default::default()
        },
      )
      .await?;

    // Remove the widget's files only once the config write has succeeded,
    // so a failed write cannot orphan them. A hand-edited pack may have
    // no directory for an entry, so a missing directory is not an error.
    let widget_dir = pack.directory_path.join(widget_name);
    ensure_direct_child(&widget_dir, &pack.directory_path)?;

    if widget_dir.is_dir() {
      fs::remove_dir_all(&widget_dir).with_context(|| {
        format!(
          "Failed to remove widget directory '{}'.",
          widget_dir.display()
        )
      })?;
    }

    Ok(())
  }
}

/// Validates a widget pack or widget name.
///
/// Mirrors the `name` schema in the client API: 2 to 28 characters,
/// lowercase letters, digits, `-` and `_`, starting with a letter or
/// digit. Names are joined into file paths, so this is what keeps them
/// from escaping the config directory.
fn validate_pack_name(name: &str) -> anyhow::Result<()> {
  let char_count = name.chars().count();

  if !(2..=28).contains(&char_count) {
    anyhow::bail!("Name must be between 2 and 28 characters.");
  }

  let mut chars = name.chars();
  let first_is_valid = chars
    .next()
    .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
  let rest_is_valid = chars.all(|c| {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
  });

  if !first_is_valid || !rest_is_valid {
    anyhow::bail!(
      "Only lowercase letters, numbers, and the characters - and _ are allowed."
    );
  }

  Ok(())
}

/// Errors unless `path` is a direct child of `base`.
///
/// Boundary check for directories derived from user-supplied names. Name
/// validation should make this unreachable, but nothing is written or
/// deleted without it.
fn ensure_direct_child(path: &Path, base: &Path) -> anyhow::Result<()> {
  let last_is_normal = matches!(
    path.components().next_back(),
    Some(std::path::Component::Normal(_))
  );

  if path.parent() != Some(base) || !last_is_normal {
    anyhow::bail!(
      "Path '{}' is not inside '{}'.",
      path.display(),
      base.display()
    );
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn accepts_valid_names() {
    for name in ["my-pack", "a1", "pack_2", "0abc", &"a".repeat(28)] {
      assert!(validate_pack_name(name).is_ok(), "{name}");
    }
  }

  #[test]
  fn rejects_invalid_names() {
    for name in [
      "",
      "a",
      &"a".repeat(29),
      "My-Pack",
      "-pack",
      "_pack",
      "..",
      "../etc",
      "a/b",
      "a\\b",
      "a b",
      "a.b",
    ] {
      assert!(validate_pack_name(name).is_err(), "{name}");
    }
  }

  #[test]
  fn direct_child_check() {
    let base = Path::new("/base");
    assert!(ensure_direct_child(&base.join("pack"), base).is_ok());
    assert!(ensure_direct_child(&base.join("a").join("b"), base).is_err());
    assert!(ensure_direct_child(&base.join(".."), base).is_err());
    assert!(ensure_direct_child(base, base).is_err());
  }
}
