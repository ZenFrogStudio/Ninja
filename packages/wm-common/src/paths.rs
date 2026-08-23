use std::{
  fs,
  path::{Path, PathBuf},
};

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
  let home = home::home_dir().context("Unable to get home directory.")?;

  migrate_legacy_dirs_in(&home.join(".glzr"), &home.join(".ninja"));

  Ok(())
}

/// Body of [`migrate_legacy_dirs`], with both roots given explicitly so it
/// can be exercised without touching the real home directory.
fn migrate_legacy_dirs_in(glzr_dir: &Path, ninja_dir: &Path) {
  // An existing `~/.ninja` means the migration has already run, or the
  // user set the directory up themselves. Either way it is theirs, and
  // copying over it would overwrite live config.
  if ninja_dir.exists() {
    return;
  }

  let sources = [
    (glzr_dir.join("glazewm"), ninja_dir.to_path_buf()),
    (glzr_dir.join("zebar"), ninja_dir.join("bar")),
  ];

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
}

/// Recursively copies a directory's contents, creating the destination if
/// needed. Existing files at the destination are overwritten.
fn copy_dir_all(from: &Path, to: &Path) -> anyhow::Result<()> {
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

  /// Temporary directory, removed when the test ends.
  struct TempDir(PathBuf);

  impl TempDir {
    fn new() -> anyhow::Result<Self> {
      let path = std::env::temp_dir()
        .join(format!("ninja-paths-test-{}", uuid::Uuid::new_v4()));

      fs::create_dir_all(&path)?;
      Ok(Self(path))
    }
  }

  impl Drop for TempDir {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.0);
    }
  }

  /// Writes a file, creating any missing parent directories.
  fn write(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }

    fs::write(path, contents)?;
    Ok(())
  }

  #[test]
  fn copies_nested_directories() -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let from = root.0.join("from");
    let to = root.0.join("to");

    write(&from.join("config.yaml"), "a")?;
    write(&from.join("nested/widget.json"), "b")?;

    copy_dir_all(&from, &to)?;

    assert_eq!(fs::read_to_string(to.join("config.yaml"))?, "a");
    assert_eq!(fs::read_to_string(to.join("nested/widget.json"))?, "b");
    Ok(())
  }

  #[test]
  fn migrates_both_legacy_dirs() -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let glzr_dir = root.0.join(".glzr");
    let ninja_dir = root.0.join(".ninja");

    write(&glzr_dir.join("glazewm/config.yaml"), "wm")?;
    write(&glzr_dir.join("glazewm/workspaces.json"), "[]")?;
    write(&glzr_dir.join("zebar/settings.json"), "bar")?;

    migrate_legacy_dirs_in(&glzr_dir, &ninja_dir);

    assert_eq!(fs::read_to_string(ninja_dir.join("config.yaml"))?, "wm");
    assert_eq!(
      fs::read_to_string(ninja_dir.join("workspaces.json"))?,
      "[]"
    );
    assert_eq!(
      fs::read_to_string(ninja_dir.join("bar/settings.json"))?,
      "bar"
    );

    // Copied, not moved.
    assert!(glzr_dir.join("glazewm/config.yaml").is_file());
    assert!(glzr_dir.join("zebar/settings.json").is_file());
    Ok(())
  }

  #[test]
  fn leaves_an_existing_ninja_dir_alone() -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let glzr_dir = root.0.join(".glzr");
    let ninja_dir = root.0.join(".ninja");

    write(&glzr_dir.join("glazewm/config.yaml"), "old")?;
    write(&ninja_dir.join("config.yaml"), "current")?;

    migrate_legacy_dirs_in(&glzr_dir, &ninja_dir);

    assert_eq!(
      fs::read_to_string(ninja_dir.join("config.yaml"))?,
      "current"
    );
    Ok(())
  }

  #[test]
  fn does_nothing_without_legacy_dirs() -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let glzr_dir = root.0.join(".glzr");
    let ninja_dir = root.0.join(".ninja");

    migrate_legacy_dirs_in(&glzr_dir, &ninja_dir);

    assert!(!ninja_dir.exists());
    Ok(())
  }

  #[test]
  fn migrates_the_bar_alone_when_only_it_exists() -> anyhow::Result<()> {
    let root = TempDir::new()?;
    let glzr_dir = root.0.join(".glzr");
    let ninja_dir = root.0.join(".ninja");

    write(&glzr_dir.join("zebar/settings.json"), "bar")?;

    migrate_legacy_dirs_in(&glzr_dir, &ninja_dir);

    assert_eq!(
      fs::read_to_string(ninja_dir.join("bar/settings.json"))?,
      "bar"
    );
    assert!(!ninja_dir.join("config.yaml").exists());
    Ok(())
  }
}
