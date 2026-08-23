#[cfg(target_os = "windows")]
use crate::platform_impl;

/// Whether the system is currently using a light theme.
///
/// Used to pick assets that sit on system-drawn surfaces, such as the
/// tray icon: a dark icon is close to invisible on a dark taskbar.
///
/// # Platform-specific
///
/// - **Windows**: Reads `SystemUsesLightTheme` from the user's
///   personalisation settings.
/// - **macOS**: Not yet implemented; always reports dark.
#[must_use]
pub fn is_light_theme() -> bool {
  #[cfg(target_os = "windows")]
  {
    platform_impl::is_light_theme()
  }
  #[cfg(not(target_os = "windows"))]
  {
    false
  }
}
