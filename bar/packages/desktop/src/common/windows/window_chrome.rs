use tauri::{Theme, WebviewWindow, WindowEvent};
use windows::Win32::{
  Foundation::HWND,
  Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR,
    DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
  },
};

/// Title bar colours for one theme, as `0x00BBGGRR`.
///
/// These mirror the `--background`, `--foreground` and `--border`
/// variables in the settings UI's stylesheet, so the title bar and the
/// page read as a single surface.
struct Chrome {
  caption: u32,
  text: u32,
  border: u32,
  is_dark: bool,
}

/// Matches the dark palette (charcoal, HSL 0 0% 8%).
const DARK: Chrome = Chrome {
  caption: 0x0014_1414,
  text: 0x00F2_F2F2,
  border: 0x0033_3333,
  is_dark: true,
};

/// Matches the light palette (HSL 0 0% 100%).
const LIGHT: Chrome = Chrome {
  caption: 0x00FF_FFFF,
  text: 0x0019_1919,
  border: 0x00E3_E3E3,
  is_dark: false,
};

/// Styles a window's title bar and keeps it in step with the OS theme.
///
/// Windows otherwise paints the title bar with the user's system accent
/// colour, which has nothing to do with the app's palette.
///
/// The attributes used here are Windows 11 only. On Windows 10 the calls
/// fail harmlessly and the system default is kept.
pub fn apply_themed_chrome(window: &WebviewWindow) -> anyhow::Result<()> {
  apply(window, window.theme().unwrap_or(Theme::Dark));

  // The theme can change while the window is open, and the title bar is
  // painted by the system rather than the webview, so it has to be
  // repainted explicitly.
  let listener = window.clone();

  window.on_window_event(move |event| {
    if let WindowEvent::ThemeChanged(theme) = event {
      apply(&listener, *theme);
    }
  });

  Ok(())
}

/// Applies the colours for a given theme.
fn apply(window: &WebviewWindow, theme: Theme) {
  let chrome = match theme {
    Theme::Light => &LIGHT,
    _ => &DARK,
  };

  let Ok(hwnd) = window.hwnd() else {
    return;
  };

  let handle = HWND(hwnd.0);

  // Switches the non-client area to its dark variant. Without this the
  // system draws light-mode controls over a dark caption.
  set_attribute(
    handle,
    DWMWA_USE_IMMERSIVE_DARK_MODE,
    &u32::from(chrome.is_dark),
  );

  set_attribute(handle, DWMWA_CAPTION_COLOR, &chrome.caption);
  set_attribute(handle, DWMWA_TEXT_COLOR, &chrome.text);
  set_attribute(handle, DWMWA_BORDER_COLOR, &chrome.border);
}

/// Sets a single DWM attribute, ignoring failures.
fn set_attribute<T>(
  handle: HWND,
  attribute: DWMWINDOWATTRIBUTE,
  value: &T,
) {
  // SAFETY: `value` outlives the call, and its size is passed explicitly.
  let result = unsafe {
    #[allow(clippy::cast_possible_truncation)]
    DwmSetWindowAttribute(
      handle,
      attribute,
      std::ptr::from_ref(value).cast(),
      std::mem::size_of::<T>() as u32,
    )
  };

  if let Err(err) = result {
    tracing::debug!("Could not set window attribute: {err}");
  }
}
