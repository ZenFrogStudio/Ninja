//! Uncloaks windows that were left cloaked by a previous WM session.
//!
//! With `hide_method: 'cloak'`, windows on non-displayed workspaces are
//! hidden by cloaking them. Neither exit path uncloaks them, so a window
//! that was hidden when the WM stopped becomes unreachable: cloaked
//! windows are excluded from Alt-Tab, and `show_all_in_taskbar: false`
//! keeps them off the taskbar too. The WM can't reclaim them either, since
//! a cloaked window reports itself as not visible.
//!
//! Recovery tool for windows already in that state. Fixing the exit paths
//! is what stops it happening again.
//!
//! # Example usage
//!
//! ```sh
//! cargo run -p wm-platform --example uncloak -- 656926 113772592
//! ```

fn main() {
  #[cfg(not(target_os = "windows"))]
  eprintln!("Cloaking is a Windows concept; nothing to do.");

  #[cfg(target_os = "windows")]
  {
    use wm_platform::{NativeWindow, NativeWindowWindowsExt};

    let handles = std::env::args()
      .skip(1)
      .map(|arg| arg.parse::<isize>())
      .collect::<Result<Vec<_>, _>>();

    let Ok(handles) = handles else {
      eprintln!("Usage: uncloak <handle> [handle...]");
      std::process::exit(1);
    };

    if handles.is_empty() {
      eprintln!("Usage: uncloak <handle> [handle...]");
      std::process::exit(1);
    }

    for handle in handles {
      let window = NativeWindow::from_handle(handle);

      if !window.is_valid() {
        println!("{handle}: no such window.");
        continue;
      }

      // Both are needed: `set_cloaked` undoes the cloak, and `show`
      // undoes an `SW_HIDE` from the other hide method. A window is only
      // ever subject to one of them, and the other call is a no-op.
      match window.set_cloaked(false) {
        Ok(()) => {
          let _ = window.show();
          let _ = window.set_taskbar_visibility(true);

          let title = window.title().unwrap_or_default();
          println!("{handle}: uncloaked ({title}).");
        }
        Err(err) => println!("{handle}: failed to uncloak: {err:?}"),
      }
    }
  }
}
