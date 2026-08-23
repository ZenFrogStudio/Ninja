use std::{fs, path::PathBuf};

use anyhow::Context;

/// Root directory for Ninja's user config and data.
///
/// Returns `~/.ninja`. The directory is not created here; callers that
/// write into it create it themselves.
pub fn ninja_dir() -> anyhow::Result<PathBuf> {
  Ok(
    home::home_dir()
      .context("Unable to get home directory.")?
      .join(".ninja"),
  )
}

/// Directory holding the bar's settings and widget packs.
///
/// Returns `~/.ninja/bar`.
pub fn bar_dir() -> anyhow::Result<PathBuf> {
  Ok(ninja_dir()?.join("bar"))
}

/// Copies config and data from the pre-rename `~/.glzr` layout into
/// `~/.ninja`, if `~/.ninja` doesn't exist yet.
///
/// `~/.glzr/glazewm` becomes `~/.ninja` and `~/.glzr/zebar` becomes
/// `~/.ninja/bar`. The old directories are copied rather than moved, so
/// going back to an older build still finds its config.
///
/// Never fatal: a failed copy leaves `~/.ninja` empty, and the app then
/// starts from its default config.
pub fn migrate_legacy_dirs() -> anyhow::Result<()> {
  let ninja_dir = ninja_dir()?;

  if ninja_dir.exists() {
    return Ok(());
  }

  let glzr_dir = home::home_dir()
    .context("Unable to get home directory.")?
    .join(".glzr");

  let sources = [
    (glzr_dir.join("glazewm"), ninja_dir.clone()),
    (glzr_dir.join("zebar"), ninja_dir.join("bar")),
  ];

  if !sources.iter().any(|(from, _)| from.is_dir()) {
    return Ok(());
  }

  for (from, to) in sources {
    if !from.is_dir() {
      continue;
    }

    match copy_dir_all(&from, &to) {
      Ok(()) => {
        tracing::info!("Migrated {} to {}.", from.display(), to.display());
      }
      Err(err) => {
        tracing::warn!("Failed to migrate {}: {err:?}", from.display());
      }
    }
  }

  Ok(())
}

/// Recursively copies a directory's contents, creating the destination if
/// needed. Existing files at the destination are overwritten.
fn copy_dir_all(from: &PathBuf, to: &PathBuf) -> anyhow::Result<()> {
  fs::create_dir_all(to)?;

  for entry in fs::read_dir(from)? {
    let entry = entry?;
    let target = to.join(entry.file_name());

    if entry.file_type()?.is_dir() {
      copy_dir_all(&entry.path(), &target)?;
    } else {
      fs::copy(entry.path(), target)?;
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn copies_nested_directories() -> anyhow::Result<()> {
    let root = std::env::temp_dir()
      .join(format!("ninja-paths-test-{}", uuid::Uuid::new_v4()));

    let from = root.join("from");
    let to = root.join("to");

    fs::create_dir_all(from.join("nested"))?;
    fs::write(from.join("config.yaml"), "a")?;
    fs::write(from.join("nested/widget.json"), "b")?;

    copy_dir_all(&from, &to)?;

    assert_eq!(fs::read_to_string(to.join("config.yaml"))?, "a");
    assert_eq!(fs::read_to_string(to.join("nested/widget.json"))?, "b");

    fs::remove_dir_all(&root)?;
    Ok(())
  }
}
