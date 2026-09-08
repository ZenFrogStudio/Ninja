use std::{
  env,
  fmt::{self, Display},
  path::Path,
  process::Command,
  str::FromStr,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
};

use anyhow::Context;
use auto_launch::AutoLaunch;
use tokio::sync::mpsc;
use tray_icon::{
  menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
  Icon, TrayIcon, TrayIconBuilder,
};
#[cfg(target_os = "windows")]
use wm_platform::DispatcherExtWindows;
use wm_platform::{Dispatcher, ThreadBound};

use crate::BarRequest;

#[derive(Debug, Clone, Eq, PartialEq)]
enum TrayMenuId {
  Settings,
  BarSettings,
  ReloadBar,
  ReloadConfig,
  ShowConfigFolder,
  #[cfg(target_os = "windows")]
  ToggleWindowAnimations,
  RunOnStartup,
  Exit,
}

impl Display for TrayMenuId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      TrayMenuId::Settings => write!(f, "settings"),
      TrayMenuId::BarSettings => write!(f, "bar_settings"),
      TrayMenuId::ReloadBar => write!(f, "reload_bar"),
      TrayMenuId::ReloadConfig => write!(f, "reload_config"),
      TrayMenuId::ShowConfigFolder => write!(f, "show_config_folder"),
      #[cfg(target_os = "windows")]
      TrayMenuId::ToggleWindowAnimations => {
        write!(f, "toggle_window_animations")
      }
      TrayMenuId::RunOnStartup => write!(f, "run_on_startup"),
      TrayMenuId::Exit => write!(f, "exit"),
    }
  }
}

impl FromStr for TrayMenuId {
  type Err = anyhow::Error;

  fn from_str(event: &str) -> Result<Self, Self::Err> {
    match event {
      "settings" => Ok(Self::Settings),
      "bar_settings" => Ok(Self::BarSettings),
      "reload_bar" => Ok(Self::ReloadBar),
      "show_config_folder" => Ok(Self::ShowConfigFolder),
      "reload_config" => Ok(Self::ReloadConfig),
      #[cfg(target_os = "windows")]
      "toggle_window_animations" => Ok(Self::ToggleWindowAnimations),
      "run_on_startup" => Ok(Self::RunOnStartup),
      "exit" => Ok(Self::Exit),
      _ => anyhow::bail!("Invalid tray menu event: {event}"),
    }
  }
}

pub struct SystemTray {
  pub config_reload_rx: mpsc::UnboundedReceiver<()>,
  pub exit_rx: mpsc::UnboundedReceiver<()>,
  _icon_thread: Option<std::thread::JoinHandle<()>>,
  _tray_icon: ThreadBound<TrayIcon>,
}

impl SystemTray {
  /// Install the system tray on the main thread after the run loop starts.
  pub fn new(
    config_path: &Path,
    dispatcher: Dispatcher,
    bar_request_tx: Option<mpsc::UnboundedSender<BarRequest>>,
  ) -> anyhow::Result<Self> {
    let (exit_tx, exit_rx) = mpsc::unbounded_channel();
    let (config_reload_tx, config_reload_rx) = mpsc::unbounded_channel();

    let animations_enabled = Arc::new(AtomicBool::new({
      #[cfg(target_os = "windows")]
      {
        dispatcher.window_animations_enabled().unwrap_or(false)
      }
      #[cfg(not(target_os = "windows"))]
      {
        false
      }
    }));

    let run_on_startup_enabled = Arc::new(AtomicBool::new(
      auto_launch_instance()
        .and_then(|auto_launch| {
          auto_launch.is_enabled().map_err(Into::into)
        })
        .unwrap_or(false),
    ));

    let tray_icon = dispatcher.dispatch_sync(|| {
      let tray_icon = Self::create_tray_icon(
        animations_enabled.load(Ordering::SeqCst),
        run_on_startup_enabled.load(Ordering::SeqCst),
      )?;

      anyhow::Ok(ThreadBound::new(tray_icon, dispatcher.clone()))
    })??;

    // Spawn thread to handle tray menu events.
    let config_path = config_path.to_owned();
    let icon_thread = std::thread::spawn(move || {
      let menu_event_rx = MenuEvent::receiver();

      while let Ok(event) = menu_event_rx.recv() {
        if let Ok(menu_event) = TrayMenuId::from_str(event.id.as_ref()) {
          if let Err(err) = Self::handle_menu_event(
            &menu_event,
            &dispatcher,
            &config_path,
            &config_reload_tx,
            &exit_tx,
            &animations_enabled,
            &run_on_startup_enabled,
            bar_request_tx.as_ref(),
          ) {
            tracing::warn!("Failed to handle tray menu event: {}", err);
          }
        }
      }
    });

    Ok(Self {
      config_reload_rx,
      exit_rx,
      _icon_thread: Some(icon_thread),
      _tray_icon: tray_icon,
    })
  }

  fn create_tray_icon(
    // LINT: `animations_enabled` is only used on Windows.
    #[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
    animations_enabled: bool,
    run_on_startup_enabled: bool,
  ) -> anyhow::Result<TrayIcon> {
    let settings_item =
      MenuItem::with_id(TrayMenuId::Settings, "Settings", true, None);

    let bar_settings_item =
      MenuItem::with_id(TrayMenuId::BarSettings, "Widgets", true, None);

    let reload_bar_item =
      MenuItem::with_id(TrayMenuId::ReloadBar, "Reload bar", true, None);

    let reload_config_item = MenuItem::with_id(
      TrayMenuId::ReloadConfig,
      "Reload config",
      true,
      None,
    );

    let config_dir_item = MenuItem::with_id(
      TrayMenuId::ShowConfigFolder,
      "Show config folder",
      true,
      None,
    );

    #[cfg(target_os = "windows")]
    let toggle_animations_item = CheckMenuItem::with_id(
      TrayMenuId::ToggleWindowAnimations,
      "Window animations",
      true,
      animations_enabled,
      None,
    );

    let run_on_startup_item = CheckMenuItem::with_id(
      TrayMenuId::RunOnStartup,
      "Run on system startup",
      true,
      run_on_startup_enabled,
      None,
    );

    let exit_item =
      MenuItem::with_id(TrayMenuId::Exit, "Exit", true, None);

    let tray_menu = Menu::new();
    tray_menu.append_items(&[
      &settings_item,
      &bar_settings_item,
      &reload_bar_item,
      &PredefinedMenuItem::separator(),
      &reload_config_item,
      &config_dir_item,
      #[cfg(target_os = "windows")]
      &toggle_animations_item,
      &run_on_startup_item,
      &PredefinedMenuItem::separator(),
      &exit_item,
    ])?;

    let icon = Self::load_icon(Self::icon_bytes())?;

    let tray_icon = TrayIconBuilder::new()
      .with_menu(Box::new(tray_menu))
      .with_tooltip(format!("Ninja v{}", env!("VERSION_NUMBER")))
      .with_icon(icon)
      .build()?;

    Ok(tray_icon)
  }

  /// Runs a bar command.
  fn run_bar_command(command: &str) -> anyhow::Result<()> {
    Self::run_bar_command_with_args(&[command])
  }

  /// Runs a multi-argument bar command.
  ///
  /// The bar shares this executable, and Tauri holds a single-instance
  /// lock, so re-invoking ourselves forwards the arguments to the running
  /// instance rather than starting a second app.
  fn run_bar_command_with_args(args: &[&str]) -> anyhow::Result<()> {
    let exe_path = env::current_exe()?;

    Command::new(&exe_path)
      .args(args)
      .spawn()
      .with_context(|| {
        format!(
          "Unable to run '{}' at {}.",
          args.join(" "),
          exe_path.display()
        )
      })?;

    Ok(())
  }

  /// Tray artwork matching the system theme.
  ///
  /// The tray sits on the taskbar, which Windows paints dark or light
  /// according to the user's setting. A dark icon on a dark taskbar is
  /// close to invisible, so the light artwork is used there.
  fn icon_bytes() -> &'static [u8] {
    // macOS menu bar icons are meant to be template images that the
    // system inverts itself, and `is_light_theme` has no implementation
    // there yet. Until both are addressed, the dark artwork is used,
    // which suits the default light menu bar.
    #[cfg(target_os = "macos")]
    {
      include_bytes!("../../../resources/assets/icon.png")
    }
    #[cfg(not(target_os = "macos"))]
    {
      if wm_platform::is_light_theme() {
        include_bytes!("../../../resources/assets/icon.png")
      } else {
        include_bytes!("../../../resources/assets/icon-light.png")
      }
    }
  }

  fn load_icon(bytes: &[u8]) -> anyhow::Result<Icon> {
    let (icon_rgba, icon_width, icon_height) = {
      let image = image::load_from_memory(bytes)
        .context("Failed to to create tray icon image from resource.")?
        .into_rgba8();

      let (width, height) = image.dimensions();
      let rgba = image.into_raw();
      (rgba, width, height)
    };

    Ok(tray_icon::Icon::from_rgba(
      icon_rgba,
      icon_width,
      icon_height,
    )?)
  }

  #[allow(clippy::too_many_arguments)]
  fn handle_menu_event(
    menu_id: &TrayMenuId,
    dispatcher: &Dispatcher,
    config_path: &Path,
    config_reload_tx: &mpsc::UnboundedSender<()>,
    exit_tx: &mpsc::UnboundedSender<()>,
    // LINT: `animations_enabled` is only used on Windows.
    #[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
    animations_enabled: &Arc<AtomicBool>,
    run_on_startup_enabled: &Arc<AtomicBool>,
    bar_request_tx: Option<&mpsc::UnboundedSender<BarRequest>>,
  ) -> anyhow::Result<()> {
    tracing::info!("Processing tray menu event: {:?}", menu_id);

    match menu_id {
      // A bar hosted in this process is asked over `bar_request_tx`.
      // Otherwise, this is the standalone WM, and the bar (if any) is a
      // separate process reached by re-invoking its binary, which its
      // single-instance handler forwards to the running instance.
      TrayMenuId::Settings => match bar_request_tx {
        Some(tx) => tx
          .send(BarRequest::OpenWmSettings)
          .context("Bar request channel is closed."),
        None => {
          Self::run_bar_command_with_args(&["settings", "--page", "wm"])
        }
      },
      TrayMenuId::BarSettings => match bar_request_tx {
        Some(tx) => tx
          .send(BarRequest::OpenSettings)
          .context("Bar request channel is closed."),
        None => Self::run_bar_command("settings"),
      },
      TrayMenuId::ReloadBar => match bar_request_tx {
        Some(tx) => tx
          .send(BarRequest::ReloadWidgets)
          .context("Bar request channel is closed."),
        None => Self::run_bar_command("reload-widgets"),
      },
      TrayMenuId::ShowConfigFolder => {
        dispatcher.open_file_explorer({
          #[cfg(target_os = "windows")]
          {
            config_path.parent().context("Invalid config path.")?
          }
          #[cfg(target_os = "macos")]
          {
            // On macOS, pass the file path directly since Finder
            // navigates one level too high with the parent directory.
            config_path
          }
        })?;

        Ok(())
      }
      TrayMenuId::ReloadConfig => {
        config_reload_tx.send(())?;
        Ok(())
      }
      #[cfg(target_os = "windows")]
      TrayMenuId::ToggleWindowAnimations => {
        let is_enabled = animations_enabled.load(Ordering::SeqCst);
        dispatcher.set_window_animations_enabled(!is_enabled)?;
        animations_enabled.store(!is_enabled, Ordering::SeqCst);
        Ok(())
      }
      TrayMenuId::RunOnStartup => {
        let is_enabled = run_on_startup_enabled.load(Ordering::SeqCst);

        if is_enabled {
          auto_launch_instance()?.disable()?;
        } else {
          auto_launch_instance()?.enable()?;
        }

        run_on_startup_enabled.store(!is_enabled, Ordering::SeqCst);
        Ok(())
      }
      TrayMenuId::Exit => {
        exit_tx.send(())?;
        Ok(())
      }
    }
  }
}

/// Creates a new [`AutoLaunch`] instance for managing auto-launch at
/// system startup.
fn auto_launch_instance() -> anyhow::Result<AutoLaunch> {
  let exe_path = std::env::current_exe()?.to_string_lossy().to_string();
  let args: [&str; 0] = [];

  #[cfg(target_os = "windows")]
  let instance = AutoLaunch::new("Ninja", &exe_path, &args);

  #[cfg(target_os = "macos")]
  let instance = AutoLaunch::new("Ninja", &exe_path, false, &args);

  Ok(instance)
}
