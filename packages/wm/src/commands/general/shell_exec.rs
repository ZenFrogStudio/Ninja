use std::path::Path;

#[cfg(target_os = "windows")]
use anyhow::Context;
#[cfg(target_os = "macos")]
use shell_util::{CommandOptions, Shell};
#[cfg(target_os = "windows")]
use wm_platform::DispatcherExtWindows;

use crate::wm_state::WmState;

/// Runs a shell command without waiting for it to finish.
///
/// Environment variables wrapped in `%` characters are expanded on
/// Windows before the command is parsed.
pub fn shell_exec(
  command: &str,
  // LINT: `hide_window` is only used on Windows.
  #[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
  hide_window: bool,
  // LINT: `state` is only used on Windows.
  #[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
  state: &WmState,
) -> anyhow::Result<()> {
  // Expand environment variables in the command string.
  let expanded_command = {
    #[cfg(target_os = "windows")]
    {
      state.dispatcher.expand_env_strings(command)?
    }
    #[cfg(target_os = "macos")]
    {
      // TODO: Expand env variables on macOS.
      command.to_string()
    }
  };

  let (program, args) = parse_command(&expanded_command)?;
  tracing::info!(
    "Parsed command program: '{}', args: '{}'.",
    program,
    args
  );

  // NOTE: The standard library's `Command::new` is not used because it
  // launches the program as a subprocess. This prevents cleanup of handles
  // held by our process (e.g. the IPC server port) until the subprocess
  // exits.
  let result = {
    #[cfg(target_os = "macos")]
    {
      Shell::spawn(
        &program,
        args.split_whitespace(),
        &CommandOptions::default(),
      )
    }
    #[cfg(target_os = "windows")]
    {
      let home_dir =
        home::home_dir().context("Unable to get home directory.")?;

      // TODO: Use `Shell::spawn` instead. `ShellExecuteExW` is still used
      // to be able to launch programs from the App Paths registry
      // (`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths`), like
      // `chrome` without it being in $PATH.
      state.dispatcher.shell_execute_ex(
        &program,
        &args,
        &home_dir,
        hide_window,
      )
    }
  };

  result.map_err(|err| {
    anyhow::anyhow!(
      "Shell exec failed for '{command}'. Make sure the program exists and is \
      accessible from your shell. Error: {err}",
    )
  })?;

  Ok(())
}

/// Parses a command string into a program name/path and arguments. If
/// the command string is a path, a file extension is required.
///
/// Environment variables are not expanded here; the caller does that so
/// the parser stays a pure string function.
///
/// This is similar to the `SHEvaluateSystemCommandTemplate` Win32
/// function. It also parses program name/path and arguments, but can't
/// handle `/` as file path delimiters and it errors for certain programs
/// (e.g. `code`).
///
/// Returns a tuple containing the program name/path and arguments.
///
/// # Examples
///
/// ```ignore
/// let (prog, args) = parse_command("code .")?;
/// assert_eq!(prog, "code");
/// assert_eq!(args, ".");
///
/// let (prog, args) = parse_command(
///   r#""C:\Program Files\Git\git-bash.exe" --cd=C:\Users\larsb\.ninja"#,
/// )?;
/// assert_eq!(prog, r#"C:\Program Files\Git\git-bash.exe"#);
/// assert_eq!(args, r#"--cd=C:\Users\larsb\.ninja"#);
/// ```
fn parse_command(command: &str) -> anyhow::Result<(String, String)> {
  let command_parts = command.split_whitespace().collect::<Vec<_>>();

  // If the command starts with double quotes, then the program name/path
  // is wrapped in double quotes (e.g. `"C:\path\to\app.exe" --flag`).
  if command.starts_with('"') {
    // Find the closing double quote (the second quote in the string).
    let (closing_index, _) =
      command.match_indices('"').nth(1).ok_or_else(|| {
        anyhow::anyhow!(
          "Shell exec failed for '{command}': command doesn't have an ending `\"`."
        )
      })?;

    return Ok((
      command[1..closing_index].to_string(),
      command[closing_index + 1..].trim().to_string(),
    ));
  }

  // The first part is the program name if it doesn't contain a slash or
  // backslash.
  if let Some(first_part) = command_parts.first() {
    if !first_part.contains(&['/', '\\'][..]) {
      let args = command_parts[1..].join(" ");
      return Ok(((*first_part).to_string(), args));
    }
  }

  let mut cumulative_path = Vec::new();

  // Lastly, iterate over the command until a valid file path is found.
  for (part_index, &part) in command_parts.iter().enumerate() {
    cumulative_path.push(part);

    if Path::new(&cumulative_path.join(" ")).is_file() {
      return Ok((
        cumulative_path.join(" "),
        command_parts[part_index + 1..].join(" "),
      ));
    }
  }

  anyhow::bail!(
    "Shell exec failed for '{command}': program path is not valid."
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn quoted_path_with_args() {
    let (program, args) =
      parse_command(r#""C:\Program Files\App\app.exe" --flag value"#)
        .expect("should parse");

    assert_eq!(program, r"C:\Program Files\App\app.exe");
    assert_eq!(args, "--flag value");
  }

  #[test]
  fn quoted_path_without_args() {
    let (program, args) =
      parse_command(r#""C:\Program Files\App\app.exe""#)
        .expect("should parse");

    assert_eq!(program, r"C:\Program Files\App\app.exe");
    assert_eq!(args, "");
  }

  #[test]
  fn quoted_path_with_quoted_args() {
    let (program, args) =
      parse_command(r#""/usr/bin/app" "with space" other"#)
        .expect("should parse");

    assert_eq!(program, "/usr/bin/app");
    assert_eq!(args, r#""with space" other"#);
  }

  #[test]
  fn unterminated_quote_errors() {
    let err = parse_command(r#""C:\Program Files\App\app.exe --flag"#)
      .expect_err("should error");

    assert!(err.to_string().contains("ending `\"`"));
  }

  #[test]
  fn bare_program() {
    let (program, args) = parse_command("code").expect("should parse");

    assert_eq!(program, "code");
    assert_eq!(args, "");
  }

  #[test]
  fn unquoted_program_with_args() {
    let (program, args) =
      parse_command("code . --new-window").expect("should parse");

    assert_eq!(program, "code");
    assert_eq!(args, ". --new-window");
  }

  #[test]
  fn unquoted_path_with_spaces_resolves_to_existing_file() {
    let dir = std::env::temp_dir()
      .join(format!("ninja-shell-exec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let file = dir.join("my app.exe");
    std::fs::write(&file, b"").expect("create temp file");

    let file_str = file.to_string_lossy().to_string();
    let result = parse_command(&format!("{file_str} --flag"));
    let _ = std::fs::remove_dir_all(&dir);

    let (program, args) = result.expect("should parse");
    assert_eq!(program, file_str);
    assert_eq!(args, "--flag");
  }

  #[test]
  fn unquoted_missing_path_errors() {
    let err = parse_command(r"C:\definitely\missing\app.exe --flag")
      .expect_err("should error");

    assert!(err.to_string().contains("program path is not valid"));
  }
}
