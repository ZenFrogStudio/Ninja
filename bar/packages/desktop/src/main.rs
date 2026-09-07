// Prevent additional console window on Windows in release mode.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{env, path::Path, sync::Arc};
#[cfg(target_os = "windows")]
use std::{path::PathBuf, thread};

use anyhow::Context;
use clap::Parser;
use tauri::{
  async_runtime::block_on, AppHandle, Emitter, Manager, RunEvent,
  WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tokio::{sync::mpsc, task};
use tracing::{debug, error, info, Level};
use tracing_subscriber::{
  fmt::{self, writer::MakeWriterExt},
  layer::SubscriberExt,
};
use wm_common::{AppCommand, Verbosity};
#[cfg(target_os = "windows")]
use wm_platform::EventLoop;

#[cfg(target_os = "windows")]
use crate::common::windows::WindowExtWindows;
use crate::{
  app_settings::AppSettings,
  asset_server::setup_asset_server,
  cli::{Cli, CliCommand, MonitorType, QueryArgs},
  monitor_state::MonitorState,
  pack_installer::PackInstaller,
  providers::{ProviderEmission, ProviderManager},
  settings_window::{open_settings_window, SettingsRoute},
  shell_state::ShellState,
  widget_factory::{WidgetFactory, WidgetOpenOptions},
  widget_pack::{MonitorSelection, WidgetPackManager, WidgetPlacement},
};

mod app_settings;
mod asset_server;
mod cli;
mod commands;
mod common;
mod config_migration;
mod monitor_state;
mod pack_installer;
mod providers;
mod settings_window;
mod shell_state;
mod widget_factory;
mod widget_pack;
mod wm_settings;

#[macro_use]
extern crate rocket;

/// Main entry point for the application.
///
/// Conditionally starts the app or runs a CLI command based on the given
/// subcommand.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // Attach to parent console on Windows in release mode.
  #[cfg(all(windows, not(debug_assertions)))]
  {
    use windows::Win32::System::Console::{
      AttachConsole, ATTACH_PARENT_PROCESS,
    };
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
  }

  // WM CLI commands (`query`, `command`, `sub`, ...) are answered here and
  // exit without starting anything. `try_parse_from` rather than `parse`,
  // because the bar has subcommands of its own that clap would otherwise
  // reject outright; those fall through to the app below, where the
  // single-instance plugin forwards them to the running instance.
  let args = env::args().collect::<Vec<_>>();

  let (wm_config_path, wm_verbosity) =
    match AppCommand::try_parse_from(&args) {
      Ok(AppCommand::Start {
        config_path,
        verbosity,
      }) => (config_path, verbosity),
      Ok(_) => return wm_cli::start(args).await,
      Err(_) => (None, Verbosity::default()),
    };

  tauri::async_runtime::set(tokio::runtime::Handle::current());

  let app = tauri::Builder::default()
    .setup(move |app| {
      task::block_in_place(|| {
        block_on(async move {
          let cli = Cli::parse();

          match cli.command() {
            CliCommand::Query(args) => output_query(app, args),
            _ => {
              let start_res = start_app(app, cli).await;

              // If unable to start Ninja, the error is fatal and a message
              // dialog is shown. Release builds run without a console, so
              // without this a user whose bar fails to start would see
              // nothing at all.
              if let Err(err) = &start_res {
                error!("{:?}", err);
                show_fatal_error_dialog(app.handle(), err);
              };

              start_res
            }
          }?;

          // The window manager runs alongside the bar in this process.
          // Started from here rather than before Tauri, so that it has an
          // `AppHandle` to bring the whole app down with when it exits.
          //
          // Windows-only. `wm-platform`'s macOS backend needs
          // `NSApplication` on the main thread, and so does tao — the two
          // can't both have it, and the WM's event loop is started on a
          // thread of its own here. Until that's resolved, macOS builds
          // are bar-only and need the window manager restored as its own
          // binary.
          #[cfg(target_os = "windows")]
          start_window_manager(
            wm_config_path,
            wm_verbosity,
            app.handle().clone(),
            app.state::<Arc<WidgetFactory>>().inner().clone(),
          )?;

          Ok(())
        })
      })
    })
    .invoke_handler(tauri::generate_handler![
      commands::widget_packs,
      commands::widget_states,
      commands::start_widget,
      commands::start_widget_preset,
      commands::stop_widget_preset,
      commands::update_widget_config,
      commands::create_widget_pack,
      commands::update_widget_pack,
      commands::delete_widget_pack,
      commands::create_widget_config,
      commands::delete_widget_config,
      commands::listen_provider,
      commands::read_wm_settings,
      commands::write_wm_settings,
      commands::unlisten_provider,
      commands::call_provider_function,
      commands::set_always_on_top,
      commands::set_skip_taskbar,
      commands::shell_exec,
      commands::shell_spawn,
      commands::shell_write,
      commands::shell_kill,
    ])
    .build(tauri::generate_context!())?;

  app.run(|app, event| {
    match &event {
      RunEvent::ExitRequested { code, .. } => {
        // The only reason the process should ever exit is an explicit
        // quit, so record it. The placeholder window is what normally
        // keeps Tauri from requesting an exit once all widgets close.
        info!("Exit requested with code {:?}.", code);

        // Deallocate any appbars on Windows.
        #[cfg(target_os = "windows")]
        {
          for (_, window) in app.webview_windows() {
            let _ = window.as_ref().window().deallocate_app_bar();
          }
        }
      }
      // Widget windows open and close routinely, but the placeholder
      // closing means the process is about to exit and is worth knowing
      // about.
      RunEvent::WindowEvent {
        label,
        event: WindowEvent::Destroyed,
        ..
      } if label == "placeholder" => {
        info!("Placeholder window was destroyed.");
      }
      _ => {}
    }
  });

  Ok(())
}

/// Query state and print to the console.
fn output_query(app: &tauri::App, args: QueryArgs) -> anyhow::Result<()> {
  match args {
    QueryArgs::Monitors => {
      let monitors = MonitorState::new(app.handle());
      cli::print_and_exit(monitors.output_str());
      Ok(())
    }
  }
}

/// Shows a blocking error dialog for a fatal startup failure.
///
/// The dialog plugin is registered as the first statement of `start_app`,
/// so it is normally available even when startup fails later. But if
/// registering the plugin is itself what failed, reaching it here would
/// panic - caught so the caller's error is still what gets returned instead
/// of being replaced by a panic.
fn show_fatal_error_dialog(app_handle: &AppHandle, err: &anyhow::Error) {
  use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

  let app_handle = app_handle.clone();
  let message = format!("{err:#}");

  // `AppHandle` isn't `UnwindSafe`, but the closure only reads it to
  // display a dialog - nothing here can leave shared state inconsistent
  // if it unwinds.
  let dialog_res =
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
      app_handle
        .dialog()
        .message(message)
        .title("Ninja failed to start")
        .kind(MessageDialogKind::Error)
        .blocking_show();
    }));

  if dialog_res.is_err() {
    error!("Failed to show the startup error dialog.");
  }
}

/// Starts Ninja - either with a specific widget or all widgets.
async fn start_app(app: &mut tauri::App, cli: Cli) -> anyhow::Result<()> {
  // Registered first, before anything else that can fail, so a fatal
  // startup error further down always has a dialog plugin available to
  // report through.
  app.handle().plugin(tauri_plugin_dialog::init())?;

  // Runs before anything reads config, so a machine upgrading from the
  // pre-rename build starts with its existing settings rather than
  // defaults.
  wm_common::migrate_legacy_dirs()?;

  let config_dir = match cli.command() {
    CliCommand::Startup(args) => args.config_dir,
    _ => None,
  }
  .unwrap_or(wm_common::bar_dir()?);

  setup_logging(&cli, &config_dir)?;
  install_panic_hook();

  // Initialize `AppSettings` in Tauri state.
  let app_settings = Arc::new(AppSettings::new(app.handle(), config_dir)?);
  app.manage(app_settings.clone());

  // Initialize `PackInstaller` in Tauri state.
  let pack_installer =
    PackInstaller::new(app.handle(), app_settings.clone())?;
  app.manage(pack_installer.clone());

  // Initialize `WidgetPackManager` in Tauri state.
  let widget_pack_manager = Arc::new(WidgetPackManager::new(
    app_settings.clone(),
    pack_installer.clone(),
  )?);
  app.manage(widget_pack_manager.clone());

  // Initialize `MonitorState` in Tauri state.
  let monitor_state = Arc::new(MonitorState::new(app.handle()));
  app.manage(monitor_state.clone());

  // Initialize `WidgetFactory` in Tauri state.
  let widget_factory = Arc::new(WidgetFactory::new(
    app.handle(),
    app_settings.clone(),
    widget_pack_manager.clone(),
    monitor_state.clone(),
  ));
  app.manage(widget_factory.clone());

  // If this is not the first instance of the app, this will emit within
  // the original instance and exit immediately. The CLI command is
  // guaranteed to be one of the open commands here.
  setup_single_instance(app, widget_factory.clone())?;

  // Start the asset server.
  setup_asset_server().await?;

  // Prevent windows from showing up in the dock on MacOS.
  #[cfg(target_os = "macos")]
  app.set_activation_policy(tauri::ActivationPolicy::Accessory);

  // Allow assets to be resolved from the config directory and the
  // marketplace download directory.
  for dir in [
    &app_settings.config_dir,
    &app_settings.marketplace_download_dir,
  ] {
    app.asset_protocol_scope().allow_directory(dir, true)?;
  }

  app.manage(ShellState::new(app.handle(), widget_factory.clone()));
  app.handle().plugin(tauri_plugin_shell::init())?;

  // Initialize `ProviderManager` in Tauri state.
  let (manager, emit_rx) = ProviderManager::new(app.handle());
  app.manage(manager.clone());

  // Open widgets based on CLI command.
  open_widgets_by_cli_command(cli, widget_factory.clone()).await?;

  listen_events(
    app.handle(),
    widget_pack_manager,
    monitor_state,
    widget_factory,
    manager,
    emit_rx,
  );

  // Placeholder window to keep the process running when all windows are
  // closed.
  create_placeholder_window(app.handle())?;

  Ok(())
}

/// Listens for events and updates state accordingly.
fn listen_events(
  app_handle: &AppHandle,
  widget_pack_manager: Arc<WidgetPackManager>,
  monitor_state: Arc<MonitorState>,
  widget_factory: Arc<WidgetFactory>,
  manager: Arc<ProviderManager>,
  mut emit_rx: mpsc::UnboundedReceiver<ProviderEmission>,
) {
  let app_handle = app_handle.clone();
  let mut widget_open_rx = widget_factory.open_tx.subscribe();
  let mut widget_close_rx = widget_factory.close_tx.subscribe();
  let mut monitors_change_rx = monitor_state.change_tx.subscribe();
  let mut widget_configs_change_rx =
    widget_pack_manager.widget_configs_change_tx.subscribe();

  task::spawn(async move {
    loop {
      let res = tokio::select! {
        Ok(widget_state) = widget_open_rx.recv() => {
          info!("Widget opened.");
          let _ = app_handle.emit("widget-opened", widget_state);
          Ok(())
        },
        Ok(widget_id) = widget_close_rx.recv() => {
          info!("Widget closed.");
          let _ = app_handle.emit("widget-closed", widget_id);
          Ok(())
        },
        Ok(monitors) = monitors_change_rx.recv() => {
          info!("Monitors changed: {} monitor(s).", monitors.len());

          // Zero monitors is a transient state (the displays powering
          // down on the idle timeout, typically), never a layout worth
          // rebuilding for. Relaunching here would close every widget and
          // open none, leaving nothing to restore when they wake.
          if monitors.is_empty() {
            info!("No monitors reported; keeping widgets as they are.");
            Ok(())
          } else {
            widget_factory.relaunch_all().await
          }
        },
        Ok((pack_id, changed_config)) = widget_configs_change_rx.recv() => {
          info!("Widget config changed.");
          widget_factory
            .relaunch_by_name(&pack_id, &changed_config.name)
            .await
        },
        Some(provider_emission) = emit_rx.recv() => {
          // Debug rather than info: the payload is the provider's
          // full state, which for the WM provider is every monitor,
          // workspace and window title on each event.
          debug!("Provider emission: {:?}", provider_emission);
          let _ = app_handle.emit("provider-emit", provider_emission.clone());
          manager.update_cache(provider_emission).await;
          Ok(())
        },
      };

      if let Err(err) = res {
        error!("{:?}", err);
      }
    }
  });
}

/// Setup single instance Tauri plugin.
fn setup_single_instance(
  app: &tauri::App,
  widget_factory: Arc<WidgetFactory>,
) -> anyhow::Result<()> {
  app.handle().plugin(tauri_plugin_single_instance::init(
    move |app_handle, args, _| {
      let widget_factory = widget_factory.clone();
      let app_handle = app_handle.clone();

      task::spawn(async move {
        let res = match Cli::try_parse_from(args) {
          Ok(cli) => match cli.command() {
            // No-op if no subcommand is provided.
            CliCommand::Empty => Ok(()),
            // Driven by the window manager's tray, so that both halves of
            // the product are controlled from a single menu.
            CliCommand::Settings(args) => open_settings_window(
              &app_handle,
              match args.page.as_deref() {
                Some("wm") => SettingsRoute::WindowManager,
                _ => SettingsRoute::Index,
              },
            ),
            CliCommand::ReloadWidgets => {
              widget_factory.relaunch_all().await
            }
            _ => open_widgets_by_cli_command(cli, widget_factory).await,
          },
          _ => Err(anyhow::anyhow!("Failed to parse CLI arguments.")),
        };

        if let Err(err) = res {
          error!("{:?}", err);
        }
      });
    },
  ))?;

  Ok(())
}

/// Opens widgets based on CLI command.
async fn open_widgets_by_cli_command(
  cli: Cli,
  widget_factory: Arc<WidgetFactory>,
) -> anyhow::Result<()> {
  let res = match cli.command() {
    CliCommand::StartWidget(args) => {
      widget_factory
        .start_widget_by_id(
          &args.pack_id,
          &args.widget_name,
          &WidgetOpenOptions::Standalone(WidgetPlacement {
            anchor: args.anchor,
            offset_x: args.offset_x,
            offset_y: args.offset_y,
            width: args.width,
            height: args.height,
            monitor_selection: match args.monitor_type {
              MonitorType::All => MonitorSelection::All,
              MonitorType::Primary => MonitorSelection::Primary,
              MonitorType::Secondary => MonitorSelection::Secondary,
            },
            dock_to_edge: Default::default(),
          }),
          false,
        )
        .await
    }
    CliCommand::StartWidgetPreset(args) => {
      widget_factory
        .start_widget_by_id(
          &args.pack_id,
          &args.widget_name,
          &WidgetOpenOptions::Preset(args.preset_name),
          false,
        )
        .await
    }
    CliCommand::Startup(_) | CliCommand::Empty => {
      widget_factory.startup().await
    }
    _ => unreachable!(),
  };

  if let Err(err) = res {
    error!("Failed to open widgets: {:?}", err);
  }

  Ok(())
}

/// Starts the window manager alongside the bar.
///
/// The platform event loop is created and run on a dedicated thread, and
/// the WM itself runs on the shared tokio runtime. Tauri owns the main
/// thread, so the event loop cannot have it.
///
/// When the WM stops — through its tray, a `wm-exit` command, or a fatal
/// error — the whole app follows it out, so that the two halves can't be
/// left half-running.
///
/// Also spawns a task that answers the WM tray's "Settings", "Widgets" and
/// "Reload bar" requests directly, so those menu items don't have to spawn
/// a second copy of this executable to reach the bar.
///
/// # Platform-specific
///
/// - **Windows**: Hosts the window manager in this process.
/// - **macOS**: Not called. See the note at the call site.
#[cfg(target_os = "windows")]
fn start_window_manager(
  config_path: Option<PathBuf>,
  verbosity: Verbosity,
  app_handle: AppHandle,
  widget_factory: Arc<WidgetFactory>,
) -> anyhow::Result<()> {
  // The event loop is built on its own thread rather than handed to it.
  // Its message window belongs to whichever thread creates it, and
  // `run` has to pump that same thread's queue, so the two can't be
  // split apart. Only the dispatcher comes back here.
  let (dispatcher_tx, dispatcher_rx) = std::sync::mpsc::channel();

  thread::Builder::new().name("wm-event-loop".into()).spawn(
    move || {
      let (event_loop, dispatcher) = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
          let _ = dispatcher_tx.send(Err(err.to_string()));
          return;
        }
      };

      // A receiver that has gone away means the caller gave up on us.
      if dispatcher_tx.send(Ok(dispatcher)).is_err() {
        return;
      }

      if let Err(err) = event_loop.run() {
        error!("Window manager event loop failed: {:?}", err);
      }
    },
  )?;

  let dispatcher = dispatcher_rx
    .recv()
    .context("Window manager event loop thread stopped early.")?
    .map_err(|err| {
      anyhow::anyhow!("Failed to start the WM event loop: {err}")
    })?;

  // Carries the tray's "Settings", "Widgets" and "Reload bar" requests to
  // the bar sharing this process, in place of spawning a second copy of
  // the executable to reach it.
  let (bar_request_tx, mut bar_request_rx) =
    mpsc::unbounded_channel::<wm::BarRequest>();

  task::spawn({
    let app_handle = app_handle.clone();

    async move {
      while let Some(request) = bar_request_rx.recv().await {
        let res = match request {
          wm::BarRequest::OpenSettings => {
            open_settings_window(&app_handle, SettingsRoute::Index)
          }
          wm::BarRequest::OpenWmSettings => {
            open_settings_window(&app_handle, SettingsRoute::WindowManager)
          }
          wm::BarRequest::ReloadWidgets => {
            widget_factory.relaunch_all().await
          }
        };

        if let Err(err) = res {
          error!("Failed to handle bar request: {:?}", err);
        }
      }
    }
  });

  // The WM gets a thread of its own too, rather than a task. Its
  // container tree is built on `Rc<RefCell<..>>`, so the future isn't
  // `Send` and can't live on the multi-threaded runtime — `block_on`
  // imposes no such bound.
  let runtime = tokio::runtime::Handle::current();

  thread::Builder::new().name("wm".into()).spawn(move || {
    // Dropped after the WM returns *or* unwinds, so a panic can't leave
    // the event loop thread parked forever.
    let _event_loop_guard = wm::EventLoopGuard(dispatcher.clone());

    runtime.block_on(async {
      if let Err(err) = wm::start_wm(
        config_path,
        verbosity,
        &dispatcher,
        Some(bar_request_tx),
      )
      .await
      {
        error!("Window manager exited with an error: {:?}", err);
        dispatcher.show_error_dialog("Fatal error", &err.to_string());
      }
    });

    // `start_wm` has returned, so its cleanup has already run and the
    // windows it was managing are restored. Take the rest of the app
    // down with it rather than leaving a bar with nothing behind it.
    info!("Window manager stopped; exiting.");
    app_handle.exit(0);
  })?;

  Ok(())
}

/// Routes panics into the log file.
///
/// Release builds run without a console (`windows_subsystem = "windows"`),
/// so the default hook writes its message to a stderr that goes nowhere.
/// Without this, a panic is indistinguishable from the process vanishing.
fn install_panic_hook() {
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

    error!(
      "Panic on thread '{}' at {}: {}\n{}",
      std::thread::current().name().unwrap_or("<unnamed>"),
      location,
      message,
      std::backtrace::Backtrace::force_capture()
    );

    default_hook(panic_info);
  }));
}

/// Initialize logging with the verbosity level specified in the CLI args.
///
/// Error logs are saved to `~/.ninja/bar/errors.log.<date>`, rotated daily
/// so that a long-running session can't bury an incident in a single
/// ever-growing file.
fn setup_logging(cli: &Cli, config_dir: &Path) -> anyhow::Result<()> {
  let log_level = match cli.command() {
    CliCommand::Startup(args) => args.verbosity.level(),
    _ => Level::INFO,
  };

  let error_writer =
    tracing_appender::rolling::daily(config_dir, "errors.log");

  let subscriber = tracing_subscriber::registry()
    .with(
      // Output to stdout with specified verbosity level.
      fmt::Layer::new()
        .with_writer(std::io::stdout.with_max_level(log_level)),
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
  info!(
    "Starting (pid {}) with log level {:?}.",
    std::process::id(),
    log_level.to_string()
  );

  Ok(())
}

/// Creates a placeholder window to prevent Tauri from automatically
/// exiting when all windows are closed.
///
/// By default, Tauri will trigger an exit request when all windows are
/// closed. Tracking issue: https://github.com/tauri-apps/tauri/issues/13511
fn create_placeholder_window(app: &tauri::AppHandle) -> tauri::Result<()> {
  let _placeholder = WebviewWindowBuilder::new(
    app,
    "placeholder",
    WebviewUrl::App("data:text/html,".into()),
  )
  .visible(false)
  .skip_taskbar(true)
  .decorations(false)
  .closable(false)
  .build()?;

  Ok(())
}
