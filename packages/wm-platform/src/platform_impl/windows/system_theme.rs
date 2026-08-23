use windows::{
  core::w,
  Win32::System::Registry::{
    RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD,
  },
};

/// Reads whether the system is using a light or dark theme.
///
/// Windows exposes this as `SystemUsesLightTheme` under the user's
/// personalisation key. The value governs the taskbar and tray, which is
/// what matters for choosing a tray icon: a dark icon is close to
/// invisible on a dark taskbar.
///
/// Defaults to dark when the value can't be read, since that has been the
/// Windows 11 default since release.
pub(crate) fn is_light_theme() -> bool {
  let mut value = 0u32;
  let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4);

  // SAFETY: `value` and `size` are valid for the duration of the call,
  // and the requested type is constrained to `REG_DWORD`.
  let result = unsafe {
    RegGetValueW(
      HKEY_CURRENT_USER,
      w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
      w!("SystemUsesLightTheme"),
      RRF_RT_REG_DWORD,
      None,
      Some(std::ptr::from_mut(&mut value).cast()),
      Some(&raw mut size),
    )
  };

  result.is_ok() && value != 0
}
