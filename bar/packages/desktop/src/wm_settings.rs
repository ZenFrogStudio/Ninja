use std::{fs, path::PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use wm_common::ClientResponseData;
use wm_ipc_client::IpcClient;

/// A single scalar setting within the WM's config file, addressed by its
/// full path through the document.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WmSettingChange {
  /// Full path to the setting, e.g.
  /// `["window_effects", "focused_window", "border", "color"]`.
  pub path: Vec<String>,

  /// Replacement value, written verbatim.
  pub value: String,
}

/// The subset of WM config the settings UI reads.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WmSettings {
  pub config_path: String,
  pub gaps: WmGapsSettings,
  pub borders: WmBorderSettings,
}

/// Border effects applied to focused and unfocused windows.
///
/// Only colour is exposed: the border is drawn by the Windows compositor
/// via `DWMWA_BORDER_COLOR`, which has no writable width, so thickness
/// can't be controlled here.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WmBorderSettings {
  pub focused_enabled: Option<String>,
  pub focused_color: Option<String>,
  pub other_enabled: Option<String>,
  pub other_color: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WmGapsSettings {
  pub scale_with_dpi: Option<String>,
  pub scale: Option<String>,
  pub inner_gap: Option<String>,
  pub outer_gap_top: Option<String>,
  pub outer_gap_right: Option<String>,
  pub outer_gap_bottom: Option<String>,
  pub outer_gap_left: Option<String>,
}

/// Asks the running WM which config file it loaded.
///
/// Queried rather than assumed, so that a `--config` override is
/// respected and the UI edits the file actually in use.
async fn config_path() -> anyhow::Result<PathBuf> {
  let mut client = IpcClient::connect().await?;
  let message = "query config-path";

  client.send(message).await?;

  let response = client
    .client_response(message)
    .await
    .context("No response to config path query.")?;

  match response.data {
    Some(ClientResponseData::ConfigPath(data)) => {
      Ok(PathBuf::from(data.config_path))
    }
    _ => anyhow::bail!(
      "Unable to determine the window manager's config path. \
       It may be running an older version."
    ),
  }
}

/// Indentation depth of a line, in spaces.
fn indent_of(line: &str) -> usize {
  line.len() - line.trim_start().len()
}

/// Splits a raw YAML value from any trailing comment.
///
/// A bare `#` search is wrong here: hex colours like `'#ff0000'` contain
/// one. YAML only starts an inline comment at a `#` preceded by
/// whitespace, and never inside a quoted scalar.
fn split_comment(raw: &str) -> (&str, &str) {
  let trimmed = raw.trim_start();

  if let Some(quote) =
    trimmed.chars().next().filter(|c| "'\"".contains(*c))
  {
    if let Some(offset) = trimmed[1..].find(quote) {
      let end = offset + 2;
      return (&trimmed[..end], trimmed[end..].trim_start());
    }
  }

  match trimmed.find(" #") {
    Some(idx) => (trimmed[..idx].trim_end(), trimmed[idx..].trim_start()),
    None => (trimmed.trim_end(), ""),
  }
}

/// Reads a scalar at a nested path, e.g.
/// `["window_effects", "focused_window", "border", "color"]`.
///
/// The final element is the key; everything before it names the enclosing
/// blocks. Paths are required because keys like `color` appear under
/// several parents.
///
/// Returns `None` when absent, which is normal: omitted keys fall back to
/// their defaults.
fn read_path(text: &str, path: &[&str]) -> Option<String> {
  let (key, sections) = path.split_last()?;
  let mut matched: Vec<usize> = Vec::new();

  for line in text.lines() {
    let content = line.trim_start();

    if content.starts_with('#') || content.is_empty() {
      continue;
    }

    let indent = indent_of(line);

    // Pop any block we've dedented out of.
    while matched.last().is_some_and(|depth| indent <= *depth) {
      matched.pop();
    }

    if matched.len() == sections.len() {
      if let Some(rest) = content.strip_prefix(&format!("{key}:")) {
        let (value, _) = split_comment(rest);

        if !value.is_empty() {
          return Some(
            value.trim_matches('\'').trim_matches('"').to_string(),
          );
        }
      }
    }

    // Descend one level when the next expected block is found.
    if let Some(next) = sections.get(matched.len()) {
      if content.starts_with(&format!("{next}:")) {
        matched.push(indent);
      }
    }
  }

  None
}

/// Rewrites a scalar at a nested path, in place.
///
/// Deliberately edits the single line rather than parsing and
/// re-serialising the document: config files are heavily commented, and a
/// round-trip through `serde_yaml` would discard every comment and all
/// formatting.
///
/// Returns `true` if the path was found and updated.
fn write_path(text: &str, path: &[&str], value: &str) -> (String, bool) {
  let Some((key, sections)) = path.split_last() else {
    return (text.to_string(), false);
  };

  let mut out = Vec::new();
  let mut matched: Vec<usize> = Vec::new();
  let mut replaced = false;

  for line in text.lines() {
    let content = line.trim_start();
    let indent = indent_of(line);

    if !content.starts_with('#') && !content.is_empty() {
      while matched.last().is_some_and(|depth| indent <= *depth) {
        matched.pop();
      }

      if !replaced && matched.len() == sections.len() {
        if let Some(rest) = content.strip_prefix(&format!("{key}:")) {
          // Preserve indentation and any trailing comment.
          let (_, trailing) = split_comment(rest);
          let comment = if trailing.is_empty() {
            String::new()
          } else {
            format!("  {trailing}")
          };

          out.push(format!("{}{key}: {value}{comment}", &line[..indent]));
          replaced = true;
          continue;
        }
      }

      if let Some(next) = sections.get(matched.len()) {
        if content.starts_with(&format!("{next}:")) {
          matched.push(indent);
        }
      }
    }

    out.push(line.to_string());
  }

  let mut result = out.join("\n");

  if text.ends_with('\n') {
    result.push('\n');
  }

  (result, replaced)
}

/// Path to a border setting for a given window group.
fn border_path<'a>(group: &'a str, key: &'a str) -> [&'a str; 4] {
  ["window_effects", group, "border", key]
}

/// Reads the WM settings exposed by the UI.
pub async fn read_wm_settings() -> anyhow::Result<WmSettings> {
  let path = config_path().await?;
  let text = fs::read_to_string(&path).with_context(|| {
    format!("Unable to read WM config at {}.", path.display())
  })?;

  Ok(WmSettings {
    config_path: path.to_string_lossy().to_string(),
    gaps: WmGapsSettings {
      scale_with_dpi: read_path(&text, &["gaps", "scale_with_dpi"]),
      scale: read_path(&text, &["gaps", "scale"]),
      inner_gap: read_path(&text, &["gaps", "inner_gap"]),
      outer_gap_top: read_path(&text, &["gaps", "outer_gap", "top"]),
      outer_gap_right: read_path(&text, &["gaps", "outer_gap", "right"]),
      outer_gap_bottom: read_path(&text, &["gaps", "outer_gap", "bottom"]),
      outer_gap_left: read_path(&text, &["gaps", "outer_gap", "left"]),
    },
    borders: WmBorderSettings {
      focused_enabled: read_path(
        &text,
        &border_path("focused_window", "enabled"),
      ),
      focused_color: read_path(
        &text,
        &border_path("focused_window", "color"),
      ),
      other_enabled: read_path(
        &text,
        &border_path("other_windows", "enabled"),
      ),
      other_color: read_path(
        &text,
        &border_path("other_windows", "color"),
      ),
    },
  })
}

/// Applies changes to the WM config and asks the WM to reload.
pub async fn write_wm_settings(
  changes: Vec<WmSettingChange>,
) -> anyhow::Result<()> {
  let path = config_path().await?;
  let original = fs::read_to_string(&path).with_context(|| {
    format!("Unable to read WM config at {}.", path.display())
  })?;

  let mut text = original.clone();

  for change in &changes {
    let path = change.path.iter().map(String::as_str).collect::<Vec<_>>();

    let (updated, replaced) = write_path(&text, &path, &change.value);

    if !replaced {
      anyhow::bail!(
        "Could not find '{}' in the config file. Add it manually first.",
        change.path.join(".")
      );
    }

    text = updated;
  }

  if text == original {
    return Ok(());
  }

  fs::write(&path, &text).with_context(|| {
    format!("Unable to write WM config at {}.", path.display())
  })?;

  // Apply immediately rather than waiting for a manual reload.
  let mut client = IpcClient::connect().await?;
  client.send("command wm-reload-config").await?;
  let _ = client.client_response("command wm-reload-config").await;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::{border_path, read_path, write_path};

  const SAMPLE: &str = "gaps:
  # Whether to scale with DPI.
  scale_with_dpi: true

  scale: 1.0
  inner_gap: '20px'

  outer_gap:
    top: '60px'
    left: '20px'

window_effects:
  focused_window:
    border:
      enabled: true
      color: '#ff0000'
  other_windows:
    border:
      enabled: true
      color: '#a1a1a1'

general:
  scale: 'unrelated'
";

  #[test]
  fn reads_nested_values() {
    assert_eq!(read_path(SAMPLE, &["gaps", "scale"]), Some("1.0".into()));
    assert_eq!(
      read_path(SAMPLE, &["gaps", "outer_gap", "top"]),
      Some("60px".into())
    );
  }

  #[test]
  fn does_not_confuse_sections() {
    // `general.scale` must not be mistaken for `gaps.scale`.
    assert_eq!(
      read_path(SAMPLE, &["general", "scale"]),
      Some("unrelated".into())
    );
  }

  #[test]
  fn distinguishes_repeated_keys_by_path() {
    // `border.color` exists under both window groups.
    assert_eq!(
      read_path(SAMPLE, &border_path("focused_window", "color")),
      Some("#ff0000".into())
    );
    assert_eq!(
      read_path(SAMPLE, &border_path("other_windows", "color")),
      Some("#a1a1a1".into())
    );
  }

  #[test]
  fn writes_the_addressed_path_only() {
    let (updated, replaced) = write_path(
      SAMPLE,
      &border_path("other_windows", "color"),
      "'#00ff00'",
    );

    assert!(replaced);
    // The focused colour must be untouched.
    assert!(updated.contains("color: '#ff0000'"));
    assert!(updated.contains("color: '#00ff00'"));
    assert!(!updated.contains("#a1a1a1"));
  }

  #[test]
  fn write_preserves_comments_and_layout() {
    let (updated, replaced) =
      write_path(SAMPLE, &["gaps", "scale"], "0.5");

    assert!(replaced);
    assert!(updated.contains("# Whether to scale with DPI."));
    assert!(updated.contains("  scale: 0.5"));
    assert!(updated.contains("  scale: 'unrelated'"));
  }

  #[test]
  fn write_reports_missing_key() {
    let (_, replaced) = write_path(SAMPLE, &["gaps", "nope"], "1");
    assert!(!replaced);
  }
}
