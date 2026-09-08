use anyhow::bail;
use serde::{Deserialize, Serialize};
use windows::Win32::{
  Foundation::HWND,
  Globalization::{LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES},
  System::SystemServices::LOCALE_NAME_MAX_LENGTH,
  UI::{
    Input::KeyboardAndMouse::GetKeyboardLayout,
    WindowsAndMessaging::{
      GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    },
  },
};

use crate::{
  common::SyncInterval,
  providers::{
    CommonProviderState, Provider, ProviderInputMsg, RuntimeType,
  },
};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardProviderConfig {
  pub refresh_interval: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardOutput {
  pub layout: String,
}

pub struct KeyboardProvider {
  config: KeyboardProviderConfig,
  common: CommonProviderState,
}

impl KeyboardProvider {
  pub fn new(
    config: KeyboardProviderConfig,
    common: CommonProviderState,
  ) -> KeyboardProvider {
    KeyboardProvider { config, common }
  }

  /// Returns the handle of the window that currently has keyboard focus.
  ///
  /// # Safety
  ///
  /// The returned handle is only a snapshot: the window may be destroyed
  /// by its owning process at any moment after this returns. The caller
  /// must only pass it to calls that tolerate a stale handle, and must
  /// not assume it stays valid across an await or a blocking call.
  unsafe fn get_focused_hwnd() -> anyhow::Result<HWND> {
    // see: https://stackoverflow.com/questions/51945835/how-to-obtain-keyboard-layout-for-microsoft-edge-and-other-windows-hosted-in-app
    let mut gui_thread_info = GUITHREADINFO {
      cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
      ..Default::default()
    };

    // SAFETY: A thread ID of 0 asks for the foreground thread, and
    // `gui_thread_info` is owned by this frame with `cbSize` set to its
    // own size, so the shell writes exactly as many bytes as fit.
    GetGUIThreadInfo(0, &mut gui_thread_info)?;

    Ok(gui_thread_info.hwndFocus)
  }

  fn run_interval(&mut self) -> anyhow::Result<KeyboardOutput> {
    // SAFETY: The focused handle is used immediately, on this thread,
    // and only by `GetWindowThreadProcessId`, which returns 0 rather
    // than faulting if the window has since been destroyed.
    // `GetKeyboardLayout` likewise tolerates a thread ID of 0.
    let keyboard_layout = unsafe {
      let hwnd = KeyboardProvider::get_focused_hwnd()?;
      GetKeyboardLayout(GetWindowThreadProcessId(hwnd, None))
    };

    let lang_id = (keyboard_layout.0 as u32) & 0xffff;
    let mut locale_name = [0; LOCALE_NAME_MAX_LENGTH as usize];

    // SAFETY: `locale_name` is an owned buffer of
    // `LOCALE_NAME_MAX_LENGTH` wide chars, the maximum a locale name can
    // occupy, and the slice passed carries that length with it.
    let result = unsafe {
      LCIDToLocaleName(
        lang_id,
        Some(&mut locale_name),
        LOCALE_ALLOW_NEUTRAL_NAMES,
      )
    };

    if result == 0 {
      bail!("Failed to get keyboard layout name.");
    }

    let layout_name =
      String::from_utf16_lossy(&locale_name[..result as usize]);

    Ok(KeyboardOutput {
      layout: layout_name,
    })
  }
}

impl Provider for KeyboardProvider {
  fn runtime_type(&self) -> RuntimeType {
    RuntimeType::Sync
  }

  fn start_sync(&mut self) {
    let mut interval = SyncInterval::new(self.config.refresh_interval);

    loop {
      crossbeam::select! {
        recv(interval.tick()) -> _ => {
          let output = self.run_interval();
          self.common.emitter.emit_output_cached(output);
        }
        recv(self.common.input.sync_rx) -> input => {
          if let Ok(ProviderInputMsg::Stop) = input {
            break;
          }
        }
      }
    }
  }
}
