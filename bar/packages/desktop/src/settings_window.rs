use anyhow::Context;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Route within the settings window to navigate to.
pub enum SettingsRoute {
  Index,
  WindowManager,
}

/// Opens the settings window, or navigates an already-open one to `route`.
pub fn open_settings_window(
  app_handle: &AppHandle,
  route: SettingsRoute,
) -> anyhow::Result<()> {
  // Get existing settings window if it's already open.
  let settings_window = app_handle.get_webview_window("settings");

  let route = match route {
    SettingsRoute::Index => "/index.html".to_string(),
    SettingsRoute::WindowManager => "/index.html#/wm".to_string(),
  };

  match &settings_window {
    None => {
      let window = WebviewWindowBuilder::new(
        app_handle,
        "settings",
        WebviewUrl::App(route.into()),
      )
      .title("Ninja Settings")
      .focused(true)
      .visible(true)
      .inner_size(900., 600.)
      .build()
      .context("Failed to build the settings window.")?;

      // Paint the title bar to match the app rather than letting
      // Windows use the system accent colour.
      #[cfg(target_os = "windows")]
      if let Err(err) =
        crate::common::windows::apply_themed_chrome(&window)
      {
        tracing::debug!("Could not style the title bar: {err}");
      }

      // Ensure window is shown and activated.
      window.show()?;
      window.set_focus()?;

      Ok(())
    }
    Some(window) => {
      window
        .eval(format!("location.replace('{}')", route))
        .context("Failed to navigate to widget edit page.")?;

      window
        .set_focus()
        .context("Failed to focus the settings window.")?;

      Ok(())
    }
  }
}
