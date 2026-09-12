use anyhow::Context;
use cocoa::{
  appkit::{NSMainMenuWindowLevel, NSWindow},
  base::id,
};
use tauri::{Runtime, Window};

pub trait WindowExtMacOs {
  fn set_above_menu_bar(&self) -> anyhow::Result<()>;
}

impl<R: Runtime> WindowExtMacOs for Window<R> {
  fn set_above_menu_bar(&self) -> anyhow::Result<()> {
    let ns_win =
      self.ns_window().context("Failed to get window handle.")? as id;

    // SAFETY: `ns_win` is the `NSWindow` backing this Tauri window, so
    // it is a live object that responds to `setLevel:`, and Tauri keeps
    // it alive for as long as the `Window` this method is called on.
    // `set_above_menu_bar` is only reached from Tauri's main thread,
    // which is where AppKit requires window mutations to happen.
    unsafe {
      ns_win.setLevel_(NSMainMenuWindowLevel as i64 + 1);
    }

    Ok(())
  }
}
