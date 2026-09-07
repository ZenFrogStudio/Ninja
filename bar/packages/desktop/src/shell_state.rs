use std::{
  collections::{hash_map::Entry, HashMap},
  path::PathBuf,
  sync::{Arc, Mutex},
};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use shell_util::{
  Buffer, ChildProcessEvent, CommandOptions, ProcessId, Shell,
  ShellExecOutput,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

use crate::{widget_factory::WidgetFactory, widget_pack::ShellPrivilege};

/// Handle for managing a spawned child process.
#[derive(Debug)]
pub struct ProcessHandle {
  /// ID of the widget that spawned the process. Only this widget may
  /// write to or kill the process.
  owner_widget_id: String,
  write_tx: mpsc::UnboundedSender<Buffer>,
  kill_tx: oneshot::Sender<()>,
  _event_task: tokio::task::JoinHandle<()>,
}

impl ProcessHandle {
  /// Errors unless `widget_id` is the widget that spawned the process.
  fn ensure_owner(
    &self,
    widget_id: &str,
    pid: ProcessId,
  ) -> anyhow::Result<()> {
    if self.owner_widget_id != widget_id {
      bail!(
        "Process with PID {pid} is not owned by widget '{widget_id}'."
      );
    }

    Ok(())
  }
}

/// Spawned child processes keyed by PID.
///
/// Every write and kill is checked against the widget that spawned the
/// process, so one widget cannot drive another widget's processes.
#[derive(Debug, Default)]
struct ProcessTable {
  children: HashMap<ProcessId, ProcessHandle>,
}

impl ProcessTable {
  /// Registers a spawned process.
  fn insert(&mut self, pid: ProcessId, handle: ProcessHandle) {
    self.children.insert(pid, handle);
  }

  /// Sends `buffer` to the stdin of the process with the given PID.
  ///
  /// Unknown PIDs are ignored. Errors if the process belongs to a
  /// different widget.
  fn write(
    &self,
    widget_id: &str,
    pid: ProcessId,
    buffer: Buffer,
  ) -> anyhow::Result<()> {
    let Some(handle) = self.children.get(&pid) else {
      return Ok(());
    };

    handle.ensure_owner(widget_id, pid)?;
    handle
      .write_tx
      .send(buffer)
      .context("Failed to send write command.")
  }

  /// Kills the process with the given PID and forgets it.
  ///
  /// Unknown PIDs are ignored. Errors if the process belongs to a
  /// different widget.
  fn kill(
    &mut self,
    widget_id: &str,
    pid: ProcessId,
  ) -> anyhow::Result<()> {
    let Entry::Occupied(entry) = self.children.entry(pid) else {
      return Ok(());
    };

    entry.get().ensure_owner(widget_id, pid)?;
    entry
      .remove()
      .kill_tx
      .send(())
      .map_err(|_| anyhow::anyhow!("Failed to send kill command."))
  }

  /// Kills every tracked process.
  fn kill_all(&mut self) {
    for (_, child) in self.children.drain() {
      let _ = child.kill_tx.send(());
    }
  }
}

/// Payload for events emitted by spawned child processes.
///
/// Sent to the client via the `shell-emit` event.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellEmission {
  pid: ProcessId,
  event: ChildProcessEvent,
}

/// Arguments for a shell command.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ShellCommandArgs {
  String(String),
  Array(Vec<String>),
}

impl From<ShellCommandArgs> for Vec<String> {
  fn from(val: ShellCommandArgs) -> Self {
    match val {
      ShellCommandArgs::String(args) => {
        args.split(' ').map(String::from).collect()
      }
      ShellCommandArgs::Array(args) => args,
    }
  }
}

impl From<ShellCommandArgs> for String {
  fn from(val: ShellCommandArgs) -> Self {
    match val {
      ShellCommandArgs::String(args) => args,
      ShellCommandArgs::Array(args) => args.join(" "),
    }
  }
}

/// Manages the state and lifecycle of shell processes.
#[derive(Debug)]
pub struct ShellState {
  app_handle: AppHandle,
  children: Arc<Mutex<ProcessTable>>,
  widget_factory: Arc<WidgetFactory>,
}

impl ShellState {
  /// Creates a new `ShellState` instance.
  pub fn new(
    app_handle: &AppHandle,
    widget_factory: Arc<WidgetFactory>,
  ) -> Self {
    Self {
      children: Arc::new(Mutex::new(ProcessTable::default())),
      app_handle: app_handle.clone(),
      widget_factory,
    }
  }

  /// Executes a command as a child process.
  ///
  /// Validates widget's shell privileges before executing the command.
  pub async fn exec(
    &self,
    widget_id: &str,
    program: &str,
    args: ShellCommandArgs,
    options: &CommandOptions,
  ) -> anyhow::Result<ShellExecOutput> {
    let resolved_program = self
      .check_shell_privilege(widget_id, program, args.clone())
      .await?;

    let resolved_program =
      resolved_program.to_str().with_context(|| {
        format!("Path to program '{program}' is not valid UTF-8.")
      })?;

    let args_vec: Vec<String> = args.into();
    let options = without_path_override(options);
    let output =
      Shell::exec(resolved_program, &args_vec, &options).await?;

    Ok(output)
  }

  /// Spawns a new child process.
  ///
  /// Validates widget's shell privileges before spawning the process.
  /// Shell events are emitted to the given widget.
  pub async fn spawn(
    &self,
    widget_id: &str,
    program: &str,
    args: ShellCommandArgs,
    options: &CommandOptions,
  ) -> anyhow::Result<ProcessId> {
    let resolved_program = self
      .check_shell_privilege(widget_id, program, args.clone())
      .await?;

    let resolved_program =
      resolved_program.to_str().with_context(|| {
        format!("Path to program '{program}' is not valid UTF-8.")
      })?;

    let args_vec: Vec<String> = args.into();
    let options = without_path_override(options);
    let mut child = Shell::spawn(resolved_program, &args_vec, &options)?;
    let app_handle = self.app_handle.clone();
    let owner_widget_id = widget_id.to_string();
    let widget_id = widget_id.to_string();
    let pid = child.pid();

    // Create channels for write and kill signals.
    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Buffer>();
    let (kill_tx, mut kill_rx) = oneshot::channel();

    // Set up event handling.
    let event_task = tokio::spawn(async move {
      loop {
        tokio::select! {
          // Process events from the child.
          Some(event) = child.events().recv() => {
            let _ = app_handle.emit_to(widget_id.clone(), "shell-emit", ShellEmission {
              pid,
              event,
            });
          }

          // Process write requests.
          Some(buffer) = write_rx.recv() => {
            if let Err(err) = child.write(buffer.as_bytes()) {
              let _ = app_handle.emit_to(widget_id.clone(), "shell-emit", ShellEmission {
                pid,
                event: ChildProcessEvent::Error(format!("Write error: {}", err)),
              });
            }
          }

          // Kill the process when signal is received.
          _ = &mut kill_rx => {
            let _ = child.kill();
            break;
          }
        }
      }
    });

    self.children.lock().unwrap().insert(
      pid,
      ProcessHandle {
        owner_widget_id,
        write_tx,
        kill_tx,
        _event_task: event_task,
      },
    );

    Ok(pid)
  }

  /// Writes data to the standard input of a running process.
  ///
  /// Errors if the process was spawned by a different widget.
  pub fn write(
    &self,
    widget_id: &str,
    pid: ProcessId,
    buffer: Buffer,
  ) -> anyhow::Result<()> {
    self.children.lock().unwrap().write(widget_id, pid, buffer)
  }

  /// Terminates a running process.
  ///
  /// Errors if the process was spawned by a different widget.
  pub fn kill(
    &self,
    widget_id: &str,
    pid: ProcessId,
  ) -> anyhow::Result<()> {
    self.children.lock().unwrap().kill(widget_id, pid)
  }

  /// Validates whether a widget has privilege to execute a program with
  /// given arguments.
  ///
  /// Resolves `program` to an absolute path using the bar's own `PATH`
  /// before matching it against the widget's privileges, so a widget
  /// cannot smuggle in its own binary under a trusted program name.
  ///
  /// Returns the resolved path if the widget has privilege, or an error
  /// otherwise.
  async fn check_shell_privilege(
    &self,
    widget_id: &str,
    program: &str,
    args: ShellCommandArgs,
  ) -> anyhow::Result<PathBuf> {
    let widget = self
      .widget_factory
      .state_by_id(widget_id)
      .await
      .with_context(|| {
        format!("Widget with ID '{widget_id}' not found.")
      })?;

    let resolved_program = resolve_program(program)?;

    let args_str: String = args.into();
    let shell_privileges = widget.config.privileges.shell_commands;

    // Check if any privilege matches the resolved program, either by raw
    // name (the common case) or by resolving the privilege's own program
    // to the same absolute path (for privileges written as a full path).
    let program_privileges: Vec<_> = shell_privileges
      .iter()
      .filter(|privilege| {
        privilege.program == program
          || resolve_program(&privilege.program).ok().as_deref()
            == Some(resolved_program.as_path())
      })
      .collect();

    if program_privileges.is_empty() {
      bail!("No shell privileges found for program '{program}'.");
    }

    if args_allowed(&program_privileges, &args_str) {
      return Ok(resolved_program);
    }

    bail!(
      "Arguments '{}' are not allowed for program '{}'. Check widget's shell privileges.",
      args_str,
      program
    )
  }
}

/// Resolves `program` to the executable that will actually run, using this
/// process's own `PATH` and never anything a widget supplied.
fn resolve_program(program: &str) -> anyhow::Result<PathBuf> {
  which::which(program)
    .with_context(|| format!("Could not resolve program '{program}'."))
}

/// Checks whether `args` matches at least one privilege's `args_regex`,
/// anchored to match the whole argument string rather than a substring.
///
/// A privilege with an empty `args_regex` only matches empty `args`. A
/// privilege whose pattern fails to compile is treated as not matching.
fn args_allowed(privileges: &[&ShellPrivilege], args: &str) -> bool {
  privileges.iter().any(|privilege| {
    if privilege.args_regex.is_empty() {
      return args.is_empty();
    }

    let anchored = format!("^(?:{})$", privilege.args_regex);

    match regex::Regex::new(&anchored) {
      Ok(re) => re.is_match(args),
      Err(_) => {
        tracing::warn!(
          "Widget shell privilege for program '{}' has an invalid \
           args_regex '{}'.",
          privilege.program,
          privilege.args_regex
        );

        false
      }
    }
  })
}

/// Drops a widget-supplied `PATH` override. Program lookup already uses
/// the resolved absolute path, so this only closes the door on tools the
/// program itself launches.
fn without_path_override(options: &CommandOptions) -> CommandOptions {
  let mut options = options.clone();

  let path_keys: Vec<String> = options
    .env
    .keys()
    .filter(|key| key.eq_ignore_ascii_case("path"))
    .cloned()
    .collect();

  for key in path_keys {
    options.env.remove(&key);
    tracing::debug!(
      "Removed widget-supplied PATH override '{key}' before spawn."
    );
  }

  options
}

impl Drop for ShellState {
  fn drop(&mut self) {
    self.children.lock().unwrap().kill_all();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Inserts a process owned by `widget_id` and returns the receiving
  /// ends of its write and kill channels.
  fn insert_process(
    table: &mut ProcessTable,
    pid: ProcessId,
    widget_id: &str,
  ) -> (mpsc::UnboundedReceiver<Buffer>, oneshot::Receiver<()>) {
    let (write_tx, write_rx) = mpsc::unbounded_channel();
    let (kill_tx, kill_rx) = oneshot::channel();

    table.insert(
      pid,
      ProcessHandle {
        owner_widget_id: widget_id.to_string(),
        write_tx,
        kill_tx,
        _event_task: tokio::spawn(async {}),
      },
    );

    (write_rx, kill_rx)
  }

  #[tokio::test]
  async fn write_rejects_other_widget() {
    let mut table = ProcessTable::default();
    let (mut write_rx, _kill_rx) =
      insert_process(&mut table, 1, "widget-a");

    let res = table.write("widget-b", 1, Buffer::Text("hi".into()));
    assert!(res.is_err());
    assert!(write_rx.try_recv().is_err());
  }

  #[tokio::test]
  async fn write_allows_owner() {
    let mut table = ProcessTable::default();
    let (mut write_rx, _kill_rx) =
      insert_process(&mut table, 1, "widget-a");

    table
      .write("widget-a", 1, Buffer::Text("hi".into()))
      .expect("owner write should succeed");

    assert!(matches!(
      write_rx.try_recv(),
      Ok(Buffer::Text(text)) if text == "hi"
    ));
  }

  #[tokio::test]
  async fn kill_rejects_other_widget() {
    let mut table = ProcessTable::default();
    let (_write_rx, mut kill_rx) =
      insert_process(&mut table, 1, "widget-a");

    assert!(table.kill("widget-b", 1).is_err());
    assert!(kill_rx.try_recv().is_err());
    assert!(table.children.contains_key(&1));
  }

  #[tokio::test]
  async fn kill_allows_owner() {
    let mut table = ProcessTable::default();
    let (_write_rx, mut kill_rx) =
      insert_process(&mut table, 1, "widget-a");

    table
      .kill("widget-a", 1)
      .expect("owner kill should succeed");

    assert!(kill_rx.try_recv().is_ok());
    assert!(!table.children.contains_key(&1));
  }

  #[tokio::test]
  async fn unknown_pid_is_ignored() {
    let mut table = ProcessTable::default();

    assert!(table
      .write("widget-a", 99, Buffer::Text("hi".into()))
      .is_ok());
    assert!(table.kill("widget-a", 99).is_ok());
  }

  fn privilege(program: &str, args_regex: &str) -> ShellPrivilege {
    ShellPrivilege {
      program: program.to_string(),
      args_regex: args_regex.to_string(),
    }
  }

  #[test]
  fn regex_must_match_whole_argument_string() {
    let status_only = [privilege("git", "status")];
    let status_refs: Vec<_> = status_only.iter().collect();

    assert!(!args_allowed(&status_refs, "-c core.sshCommand=x status"));
    assert!(args_allowed(&status_refs, "status"));

    let anything = [privilege("git", ".*")];
    let anything_refs: Vec<_> = anything.iter().collect();

    assert!(args_allowed(&anything_refs, "-c core.sshCommand=x status"));
  }

  #[test]
  fn path_override_is_stripped() {
    let mut options = CommandOptions::default();
    options.env.insert("PATH".to_string(), "evil".to_string());
    options.env.insert("Path".to_string(), "evil".to_string());
    options.env.insert("path".to_string(), "evil".to_string());
    options
      .env
      .insert("OTHER_VAR".to_string(), "keep-me".to_string());

    let sanitized = without_path_override(&options);

    assert!(!sanitized.env.contains_key("PATH"));
    assert!(!sanitized.env.contains_key("Path"));
    assert!(!sanitized.env.contains_key("path"));
    assert_eq!(
      sanitized.env.get("OTHER_VAR"),
      Some(&"keep-me".to_string())
    );
  }

  #[cfg(windows)]
  #[test]
  fn resolves_program_on_path() {
    let resolved = resolve_program("cmd").expect("cmd should be on PATH");

    assert!(resolved.is_absolute());
    assert!(resolved
      .to_string_lossy()
      .to_lowercase()
      .ends_with("cmd.exe"));
  }
}
