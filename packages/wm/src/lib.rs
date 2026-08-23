#![warn(clippy::all, clippy::pedantic)]
#![feature(iterator_try_collect)]

#[cfg(target_os = "macos")]
use std::io::IsTerminal;
use std::{env, path::PathBuf, process, time::Duration};

use anyhow::{Context, Error};
use tokio::{process::Command, signal};
use tracing::Level;
use tracing_subscriber::{
  fmt::{self, writer::MakeWriterExt},
  layer::SubscriberExt,
};
use wm_common::{AppCommand, InvokeCommand, Verbosity, WmEvent};
#[cfg(target_os = "macos")]
use wm_platform::DispatcherExtMacOs;
use wm_platform::{
  Dispatcher, DisplayListener, EventLoop, KeybindingListener,
  MouseEventKind, MouseListener, PlatformEvent, SingleInstance,
  WindowListener,
};

use crate::{
  ipc_server::IpcServer, sys_tray::SystemTray, user_config::UserConfig,
  wm::WindowManager,
};

mod commands;
mod events;
mod ipc_server;
mod models;
mod pending_sync;
mod sys_tray;
mod traits;
mod user_config;
mod wm;
mod wm_state;
mod workspace_store;

#[cfg(test)]
mod test_utils;

/// Talks to the WM's IPC server from inside its own process, for
/// callers hosted alongside it.
pub use ipc_server::LocalIpcClient;

/// Runs the WM as a standalone process.
///
/// Conditionally starts the WM or runs a CLI command based on the given
/// subcommand. Owns the main thread for the lifetime of the WM.
///
/// Callers that host the WM alongside another event loop should drive
/// [`start_wm`] themselves instead, and give the platform event loop a
/// thread of its own.
///
/// # Errors
///
/// Returns an error if the WM fails to start, exits abnormally, or if the
/// CLI subcommand fails.
pub fn run() -> anyhow::Result<()> {
  let args = std::env::args().collect::<Vec<_>>();
  let app_command = AppCommand::parse_with_default(&args);

  if let AppCommand::Start {
    config_path,
    verbosity,
  } = app_command
  {
    let rt = tokio::runtime::Runtime::new()?;
    let (event_loop, dispatcher) = EventLoop::new()?;

    let task_handle = std::thread::spawn(move || {
      // Dropped after the WM returns *or* unwinds, so a panic can't leave
      // the main thread blocked in `EventLoop::run` forever.
      let _event_loop_guard = EventLoopGuard(dispatcher.clone());

      rt.block_on(async {
        let start_res =
          start_wm(config_path, verbosity, &dispatcher).await;

        if let Err(err) = &start_res {
          // If unable to start the WM, the error is fatal and a message
          // dialog is shown.
          tracing::error!("{:?}", err);
          dispatcher.show_error_dialog("Fatal error", &err.to_string());
        }

        start_res
      })
    });

    // Run event loop (blocks until shutdown). This must be on the main
    // thread for macOS compatibility.
    event_loop.run()?;

    // Wait for clean exit of the WM. A panic is reported rather than
    // re-raised here, so that the panic hook's log entry stays the last
    // thing written.
    if let Ok(start_res) = task_handle.join() {
      start_res
    } else {
      tracing::error!("WM thread panicked; see the panic entry above.");
      anyhow::bail!("The window manager exited unexpectedly.")
    }
  } else {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(wm_cli::start(args))
  }
}

/// Stops the event loop when the WM thread ends.
///
/// The event loop runs on the main thread and only returns once it has
/// been told to stop. Doing that from a `Drop` rather than inline means it
/// also happens when the WM thread unwinds from a panic.
pub struct EventLoopGuard(pub Dispatcher);

impl Drop for EventLoopGuard {
  fn drop(&mut self) {
    if let Err(err) = self.0.stop_event_loop() {
      // Forcefully exit the process to ensure the event loop is stopped.
      tracing::error!("Failed to stop event loop gracefully: {}", err);
      process::exit(1);
    }
  }
}

/// Runs the window manager until it is told to exit.
///
/// Expects `dispatcher` to belong to a [`wm_platform::EventLoop`] that is
/// already running on a thread of its own, and blocks the calling task for
/// the lifetime of the WM.
///
/// # Errors
///
/// Returns an error if startup fails — another instance is already
/// running, the user config is invalid, or a platform listener can't be
/// registered — or if the main loop hits an unrecoverable error.
#[allow(clippy::too_many_lines)]
pub async fn start_wm(
  config_path: Option<PathBuf>,
  verbosity: Verbosity,
  dispatcher: &Dispatcher,
) -> anyhow::Result<()> {
  // Non-fatal: a host that already runs its own logging has installed a
  // subscriber for the whole process, and only one can be set. The WM's
  // output goes to that log instead of its own, which is what a single
  // process should do anyway. Not worth refusing to start over.
  if let Err(err) = setup_logging(&verbosity) {
    tracing::debug!("Using the host's existing log subscriber: {err}");
  }

  install_panic_hook();

  // Ensure that only one instance of the WM is running.
  let _single_instance = SingleInstance::new()?;

  #[cfg(target_os = "macos")]
  {
    if !dispatcher.has_ax_permission(true) {
      anyhow::bail!(
        "Accessibility permissions are not granted. In System Preferences, \
         go to Privacy & Security > Accessibility and enable Ninja."
      );
    }
  }

  // Parse and validate user config.
  let mut config = UserConfig::new(config_path)?;

  // Add application icon to system tray.
  let mut tray = SystemTray::new(&config.path, dispatcher.clone())?;

  let mut wm = WindowManager::new(&mut config, dispatcher.clone())?;

  let mut ipc_server = IpcServer::start()?;

  // On Windows, start watcher process for restoring hidden windows on
  // crash. macOS' hidden windows are always accessible.
  //
  // The handle is kept for the lifetime of the WM. Dropping it leaves the
  // process object unreaped, which keeps the watcher's executable locked
  // and its entry in the process list long after it has exited.
  #[cfg(target_os = "windows")]
  let mut watcher_process = match start_watcher_process() {
    Ok(child) => Some(child),
    Err(err) => {
      tracing::warn!(
        "Failed to start watcher process: {err}{}",
        cfg!(debug_assertions)
          .then_some(".\n Run `cargo build -p wm-watcher` to build it.")
          .unwrap_or_default()
      );

      None
    }
  };

  // On macOS, update the current process' PATH variable so that
  // `shell-exec` can resolve programs defined in the shell's PATH. Skip if
  // running via a terminal.
  #[cfg(target_os = "macos")]
  if !std::io::stdin().is_terminal() {
    update_path_env();
  }

  // Start listening for platform events after populating initial state.
  let mut window_listener = WindowListener::new(dispatcher)?;
  let mut display_listener = DisplayListener::new(dispatcher)?;
  let mut mouse_listener = MouseListener::new(
    if config.value.general.focus_follows_cursor {
      &[MouseEventKind::Move, MouseEventKind::LeftButtonUp]
    } else {
      &[MouseEventKind::LeftButtonUp]
    },
    dispatcher,
  )?;
  let mut keybinding_listener = KeybindingListener::new(
    &config
      .active_keybinding_configs(&[], false)
      .flat_map(|kb| kb.bindings)
      .collect::<Vec<_>>(),
    dispatcher,
  )?;

  // Run user's startup commands.
  if let Err(err) = wm.process_commands(
    &config.value.general.startup_commands.clone(),
    None,
    &mut config,
  ) {
    tracing::error!("{:?}", err);
    dispatcher.show_error_dialog("Non-fatal error", &err.to_string());
  }

  // Create an interval for periodically cleaning up invalid windows.
  let mut cleanup_interval = tokio::time::interval(Duration::from_secs(5));
  cleanup_interval
    .set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

  loop {
    let res = tokio::select! {
      _ = signal::ctrl_c() => {
        tracing::info!("Received SIGINT signal.");
        break;
      },
      Some(()) = wm.exit_rx.recv() => {
        tracing::info!("Exiting through WM command.");
        break;
      },
      Some(()) = tray.exit_rx.recv() => {
        tracing::info!("Exiting through system tray.");
        break;
      },
      Some(event) = mouse_listener.next_event() => {
        tracing::debug!("Received mouse event: {:?}", event);
        wm.process_event(PlatformEvent::Mouse(event), &mut config)
      },
      Some(event) = window_listener.next_event() => {
        tracing::debug!("Received window event: {:?}", event);
        wm.process_event(PlatformEvent::Window(event), &mut config)
      },
      Some(()) = display_listener.next_event() => {
        tracing::debug!("Received display settings changed event.");
        wm.process_event(PlatformEvent::DisplaySettingsChanged, &mut config)
      },
      Some(event) = keybinding_listener.next_event() => {
        tracing::debug!("Received keyboard event: {:?}", event);
        wm.process_event(PlatformEvent::Keybinding(event), &mut config)
      }
      _ = cleanup_interval.tick() => {
        if wm.state.is_paused {
          Ok(())
        } else {
          wm.state.cleanup_invalid_windows()
        }
      },
      Some((
        message,
        response_tx,
        disconnection_tx
      )) = ipc_server.message_rx.recv() => {
        // Debug rather than info: the bar re-queries the full state on
        // every WM event, which is several messages per event and buries
        // everything else in the log.
        tracing::debug!("Received IPC message: {:?}", message);

        if let Err(err) = ipc_server.process_message(
          message,
          &response_tx,
          &disconnection_tx,
          &mut wm,
          &mut config,
        ) {
          tracing::error!("{:?}", err);
        }

        Ok(())
      },
      Some(wm_event) = wm.event_rx.recv() => {
        tracing::debug!("Received WM event: {:?}", wm_event);

        // Disable mouse listener when the WM is paused.
        if let WmEvent::PauseChanged { is_paused } = wm_event {
          let _ = mouse_listener.enable(!is_paused);
        }

        // Update keybinding and mouse listeners on config changes.
        if matches!(
          wm_event,
          WmEvent::UserConfigChanged { .. }
            | WmEvent::BindingModesChanged { .. }
            | WmEvent::PauseChanged { .. }
        ) {
          keybinding_listener.update(
            &config
              .active_keybinding_configs(&wm.state.binding_modes, false)
              .flat_map(|kb| kb.bindings)
              .collect::<Vec<_>>(),
          );

          // Logged rather than propagated: a `?` here would return out of
          // `start_wm` and take the whole process down over a transient
          // failure to reapply mouse config.
          if let Err(err) = mouse_listener.set_enabled_events(
            if config.value.general.focus_follows_cursor {
              &[MouseEventKind::Move, MouseEventKind::LeftButtonUp]
            } else {
              &[MouseEventKind::LeftButtonUp]
            },
          ) {
            tracing::error!("Failed to update mouse listener: {:?}", err);
          }
        }

        if let Err(err) = ipc_server.process_event(wm_event) {
          tracing::error!("{:?}", err);
        }

        Ok(())
      },
      Some(()) = tray.config_reload_rx.recv() => {
        wm.process_commands(
          &vec![InvokeCommand::WmReloadConfig],
          None,
          &mut config,
        ).map(|_| ())
      },
    };

    if let Err(err) = res {
      tracing::error!("{:?}", err);
      dispatcher.show_error_dialog("Non-fatal error", &err.to_string());
    }
  }

  tracing::info!("Window manager shutting down.");
  wm.cleanup(&mut config, &mut ipc_server);

  // Reap the watcher so its process object is released. It normally exits
  // on its own once the IPC connection closes, but waiting here is what
  // actually closes our handle to it.
  #[cfg(target_os = "windows")]
  if let Some(mut watcher) = watcher_process.take() {
    let _ = watcher.start_kill();

    if tokio::time::timeout(Duration::from_secs(3), watcher.wait())
      .await
      .is_err()
    {
      tracing::warn!("Timed out waiting for the watcher to exit.");
    }
  }

  Ok(())
}

/// Routes panics into the log file.
///
/// Release builds run without a console (`windows_subsystem = "windows"`),
/// so the default hook writes its message to a stderr that goes nowhere.
/// Without this, a panic is indistinguishable from the process vanishing.
pub fn install_panic_hook() {
  let default_hook = std::panic::take_hook();

  std::panic::set_hook(Box::new(move |panic_info| {
    let payload = panic_info.payload();

    let message = payload
      .downcast_ref::<&str>()
      .copied()
      .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
      .unwrap_or("<non-string panic payload>");

    let location = panic_info
      .location()
      .map_or_else(|| "unknown location".to_string(), ToString::to_string);

    tracing::error!(
      "Panic on thread '{}' at {}: {}\n{}",
      std::thread::current().name().unwrap_or("<unnamed>"),
      location,
      message,
      std::backtrace::Backtrace::force_capture()
    );

    default_hook(panic_info);
  }));
}

/// Initialize logging with the specified verbosity level.
///
/// Error logs are saved to `~/.ninja/errors.log.<date>`, rotated daily so
/// that a long-running session can't bury an incident in a single
/// ever-growing file.
///
/// # Errors
///
/// Returns an error if the home directory can't be resolved, or if a
/// global subscriber has already been installed for this process.
pub fn setup_logging(verbosity: &Verbosity) -> anyhow::Result<()> {
  let error_log_dir = wm_common::ninja_dir()?;

  let error_writer =
    tracing_appender::rolling::daily(error_log_dir, "errors.log");

  let subscriber = tracing_subscriber::registry()
    .with(
      // Output to stdout with specified verbosity level.
      fmt::Layer::new()
        .with_writer(std::io::stdout.with_max_level(verbosity.level())),
    )
    .with(
      // Output to error log file.
      fmt::Layer::new()
        // Info rather than error: release builds run without a console
        // (`windows_subsystem = "windows"`), so this file is the only
        // record a user has when something goes wrong. Verbose
        // payloads are logged at debug and stay out of it.
        .with_writer(error_writer.with_max_level(Level::INFO)),
    );

  tracing::subscriber::set_global_default(subscriber)?;

  // The PID makes overlapping sessions in one log file separable.
  tracing::info!(
    "Starting WM (pid {}) with log level {:?}.",
    std::process::id(),
    verbosity.level().to_string()
  );

  log_ui_access_state();

  Ok(())
}

/// Records whether the build has `UIAccess`, and what is lost without it.
///
/// Without it, `SetWindowPos` against a window owned by an elevated
/// process fails, and the only symptom is a "Failed to set window
/// position" warning per redraw. Stating it once at startup makes that
/// diagnosable.
///
/// # Platform-specific
///
/// - **Windows**: Reports the state of the `ui_access` feature. Windows
///   only grants the privilege to a signed binary in a secure location, so
///   an unsigned build has it compiled out (see `docs/signing.md`).
/// - **macOS**: No-op. `UIAccess` is a Windows concept.
fn log_ui_access_state() {
  #[cfg(all(target_os = "windows", feature = "ui_access"))]
  tracing::info!("UIAccess: enabled.");

  #[cfg(all(target_os = "windows", not(feature = "ui_access")))]
  tracing::info!(
    "UIAccess: disabled. Windows owned by elevated processes cannot be \
     moved or resized. See docs/signing.md to enable it."
  );
}

/// Launches watcher binary (Windows-only). This is a separate process that
/// is responsible for restoring hidden windows in case the main WM process
/// crashes.
///
/// This assumes the watcher binary exists in the same directory as the
/// WM binary.
#[allow(unused)]
fn start_watcher_process() -> anyhow::Result<tokio::process::Child, Error>
{
  let watcher_path = env::current_exe()?
    .parent()
    .context("Failed to resolve path to the watcher process.")?
    .join("ninja-watcher");

  Command::new(&watcher_path)
    .spawn()
    .context("Failed to start watcher process.")
}

/// Updates the current process' PATH by querying the login shell.
///
/// Apps launched outside a terminal (Spotlight, Finder, login items)
/// inherit a PATH that only contains `/usr/bin:/bin:/usr/sbin:/sbin`. This
/// causes `shell-exec` to fail for binaries that aren't in the system
/// PATH.
#[cfg(target_os = "macos")]
fn update_path_env() {
  let shell =
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

  // Use `-l` and `-i` (login + interactive) so that both profile and rc
  // files are sourced.
  let path_var = match std::process::Command::new(&shell)
    .args(["-lic", "printf '%s' \"$PATH\""])
    .output()
  {
    Ok(output) if output.status.success() => {
      String::from_utf8(output.stdout)
        .ok()
        .filter(|path| !path.is_empty())
    }
    _ => None,
  };

  if let Some(path) = path_var {
    std::env::set_var("PATH", path);
  } else {
    tracing::warn!(
      "Failed to query login shell for PATH. Keeping existing PATH."
    );
  }
}
