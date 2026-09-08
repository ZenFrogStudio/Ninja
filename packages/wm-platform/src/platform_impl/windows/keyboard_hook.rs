use std::cell::Cell;

use windows::Win32::{
  Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
  UI::{
    Input::KeyboardAndMouse::{
      GetKeyState, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL,
      VK_RMENU, VK_RSHIFT, VK_RWIN,
    },
    WindowsAndMessaging::{
      CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
      KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    },
  },
};

use crate::{Dispatcher, Key, KeyCode};

/// Callback stored in [`HOOK`] for intercepting keyboard events.
type HookCallback = Box<dyn Fn(KeyEvent) -> bool>;

thread_local! {
  /// Stores the hook callback for the current thread.
  ///
  /// The hook callback is called for every keyboard event and returns
  /// `true` if the event should be intercepted.
  static HOOK: Cell<Option<HookCallback>> = Cell::default();
}

/// A key event received from the keyboard hook.
#[derive(Clone, Debug)]
pub struct KeyEvent {
  /// The key that was pressed or released.
  pub key: Key,

  /// Key code that generated this event.
  #[allow(dead_code)]
  pub key_code: KeyCode,

  /// Whether the event is for a key press or release.
  pub is_keypress: bool,
}

impl KeyEvent {
  /// Gets whether the specified key is currently pressed.
  #[allow(clippy::unused_self)]
  pub fn is_key_down(&self, key: Key) -> bool {
    match key {
      Key::Cmd | Key::Win => {
        Self::is_key_down_raw(VK_LWIN.0)
          || Self::is_key_down_raw(VK_RWIN.0)
      }
      Key::Alt => {
        Self::is_key_down_raw(VK_LMENU.0)
          || Self::is_key_down_raw(VK_RMENU.0)
      }
      Key::Ctrl => {
        Self::is_key_down_raw(VK_LCONTROL.0)
          || Self::is_key_down_raw(VK_RCONTROL.0)
      }
      Key::Shift => {
        Self::is_key_down_raw(VK_LSHIFT.0)
          || Self::is_key_down_raw(VK_RSHIFT.0)
      }
      _ => {
        if let Ok(key_code) = KeyCode::try_from(key) {
          Self::is_key_down_raw(key_code.0)
        } else {
          false
        }
      }
    }
  }

  /// Gets whether the specified key is currently down using the raw key
  /// code.
  fn is_key_down_raw(key: u16) -> bool {
    // SAFETY: `GetKeyState` takes a virtual-key code by value and touches
    // no caller memory. An out-of-range code returns zero rather than
    // faulting.
    unsafe { (GetKeyState(key.into()) & 0x80) == 0x80 }
  }
}

/// A system-wide low-level keyboard hook.
#[derive(Debug)]
pub struct KeyboardHook {
  /// Held as `isize` rather than `HHOOK`, which wraps a raw pointer and
  /// so can't cross the thread boundary the hook is set on.
  handle: isize,
  dispatcher: Dispatcher,
}

impl KeyboardHook {
  /// Creates an instance of `KeyboardHook`.
  ///
  /// The callback is called for every keyboard event and returns `true` if
  /// the event should be intercepted.
  ///
  /// # Panics
  ///
  /// Panics when attempting to register multiple hooks on the dispatcher's
  /// thread.
  pub fn new<F>(
    callback: F,
    dispatcher: &Dispatcher,
  ) -> crate::Result<Self>
  where
    F: Fn(KeyEvent) -> bool + Send + Sync + 'static,
  {
    let handle = dispatcher.dispatch_sync(move || {
      HOOK.with(|state| {
        assert!(
          state.take().is_none(),
          "Only one keyboard hook can be registered on the dispatcher's thread."
        );

        state.set(Some(Box::new(callback)));
      });

      // Returned as `isize`, since `HHOOK` wraps a raw pointer and so
      // isn't `Send`.
      // SAFETY: This runs on the dispatcher's thread, which has the
      // message loop that a `WH_KEYBOARD_LL` hook needs. `hook_proc` is a
      // `'static` function with the signature Windows expects, and the
      // callback it reads was installed on this same thread just above.
      unsafe {
        SetWindowsHookExW(
          WH_KEYBOARD_LL,
          Some(Self::hook_proc),
          HINSTANCE::default(),
          0,
        )
      }
      .map(|hook| hook.0 as isize)
    })??;

    Ok(Self {
      handle,
      dispatcher: dispatcher.clone(),
    })
  }

  /// Terminates the keyboard hook by unregistering it.
  pub fn terminate(&mut self) -> crate::Result<()> {
    // SAFETY: `handle` was returned by `SetWindowsHookExW` in `new` and is
    // owned by this `KeyboardHook`, which unhooks it only here. A second
    // call is reported as an error rather than unhooking someone else's
    // hook.
    unsafe {
      UnhookWindowsHookEx(HHOOK(self.handle as *mut std::ffi::c_void))
    }?;

    // Dispatch cleanup to the event loop thread since the callback
    // is stored in a thread-local on that thread.
    let _ = self.dispatcher.dispatch_async(|| {
      HOOK.with(|state| {
        state.take();
      });
    });

    Ok(())
  }

  /// Hook procedure for keyboard events.
  ///
  /// For use with `SetWindowsHookExW`.
  extern "system" fn hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
  ) -> LRESULT {
    // If the code is less than zero, the hook procedure must pass the hook
    // notification directly to other applications.
    if code != 0 {
      // SAFETY: Windows calls this procedure and owns `code`, `wparam` and
      // `lparam`; passing them along unchanged is what the API asks for. A
      // `None` hook handle is accepted and means "start from this hook".
      return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    // Get struct with the keyboard input event.
    // SAFETY: With a `code` of `HC_ACTION`, Windows guarantees that
    // `lparam` points to a `KBDLLHOOKSTRUCT` that is valid for the
    // duration of this call, so the copy out of it is in bounds.
    let input = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };

    #[allow(clippy::cast_possible_truncation)]
    let key_code = KeyCode(input.vkCode as u16);
    #[allow(clippy::cast_possible_truncation)]
    let is_keypress =
      wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;

    let Ok(key) = Key::try_from(key_code) else {
      // SAFETY: As above, the arguments are the ones Windows passed in and
      // are forwarded unchanged.
      return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };

    let key_event = KeyEvent {
      key,
      key_code,
      is_keypress,
    };

    let should_intercept = HOOK.with(|state| {
      if let Some(callback) = state.take() {
        let result = callback(key_event);
        state.set(Some(callback));
        result
      } else {
        false
      }
    });

    if should_intercept {
      return LRESULT(1);
    }

    // SAFETY: As above, the arguments are the ones Windows passed in and
    // are forwarded unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
  }
}

impl Drop for KeyboardHook {
  fn drop(&mut self) {
    let _ = self.terminate();
  }
}
