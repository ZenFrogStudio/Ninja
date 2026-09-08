// The `windows_interface::interface` expansions trip this lint, and the
// generated code isn't ours to change.
#![allow(clippy::transmute_ptr_to_ptr)]

use std::cell::RefCell;

use windows::{
  core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT},
  Win32::{
    System::Com::{
      CoCreateInstance, CoInitializeEx, CoUninitialize, IServiceProvider,
      CLSCTX_ALL, CLSCTX_SERVER, COINIT_APARTMENTTHREADED,
    },
    UI::Shell::{ITaskbarList2, TaskbarList},
  },
};

/// COM class identifier (CLSID) for the Windows Shell that implements the
/// `IServiceProvider` interface.
const CLSID_IMMERSIVE_SHELL: GUID =
  GUID::from_u128(0xC2F03A33_21F5_47FA_B4BB_156362A2F239);

thread_local! {
  /// Manages per-thread COM initialization. COM must be initialized on each
  /// thread that uses it, so we store this in thread-local storage to handle
  /// the setup and cleanup automatically.
  ///
  /// Wrapped in `RefCell` to allow mutation via `COM_INIT.borrow_mut()`.
  pub(crate) static COM_INIT: RefCell<ComInit> = RefCell::new(ComInit::new());
}

pub(crate) struct ComInit {
  service_provider: Option<IServiceProvider>,
  application_view_collection: Option<IApplicationViewCollection>,
  taskbar_list: Option<ITaskbarList2>,
}

impl ComInit {
  /// Initializes COM on the current thread with apartment threading model.
  /// `COINIT_APARTMENTTHREADED` is required for shell COM objects.
  ///
  /// # Panics
  ///
  /// Panics if COM initialization fails. This is typically only possible
  /// if COM is already initialized with an incompatible threading model.
  #[must_use]
  pub(crate) fn new() -> Self {
    // SAFETY: `COM_INIT` builds one `ComInit` per thread, so this is the
    // first initialization on this thread, and the matching
    // `CoUninitialize` runs in `Drop`.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
      .ok()
      .expect("Unable to initialize COM.");

    // SAFETY: COM was just initialized on this thread, and the CLSID and
    // the interface being requested are compile-time constants that
    // match each other.
    let service_provider = unsafe {
      CoCreateInstance(&CLSID_IMMERSIVE_SHELL, None, CLSCTX_ALL)
    }
    .ok();

    // SAFETY: `provider` is a live interface created just above on this
    // COM-initialized thread, and the IID is a compile-time constant
    // matching the interface the result is stored as.
    let application_view_collection = service_provider.as_ref().and_then(
      |provider: &IServiceProvider| unsafe {
        provider.QueryService(&IApplicationViewCollection::IID).ok()
      },
    );

    // SAFETY: COM was initialized on this thread above, and the CLSID
    // and the interface being requested are compile-time constants that
    // match each other.
    let taskbar_list =
      unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_SERVER) }.ok();

    Self {
      service_provider,
      application_view_collection,
      taskbar_list,
    }
  }

  /// Returns an instance of `IApplicationViewCollection`.
  pub(crate) fn application_view_collection(
    &self,
  ) -> crate::Result<&IApplicationViewCollection> {
    self.application_view_collection.as_ref().ok_or_else(|| {
      crate::Error::Platform(
        "Failed to query for `IApplicationViewCollection` instance."
          .to_string(),
      )
    })
  }

  /// Returns an instance of `ITaskbarList2`.
  pub(crate) fn taskbar_list(&self) -> crate::Result<&ITaskbarList2> {
    self.taskbar_list.as_ref().ok_or_else(|| {
      crate::Error::Platform(
        "Unable to create `ITaskbarList2` instance.".to_string(),
      )
    })
  }

  /// Refreshes cached COM interfaces.
  ///
  /// Called automatically by `with_retry` when COM operations fail due to
  /// stale interface pointers (e.g. after Explorer restarts).
  pub(crate) fn refresh(&mut self) {
    // Re-create the service provider.
    // SAFETY: A `ComInit` only exists on a thread where `new` ran
    // `CoInitializeEx`, and the CLSID and the interface being requested
    // are compile-time constants that match each other.
    self.service_provider = unsafe {
      CoCreateInstance(&CLSID_IMMERSIVE_SHELL, None, CLSCTX_ALL)
    }
    .ok();

    // Re-create the application view collection.
    // SAFETY: `provider` is the interface created just above on this
    // COM-initialized thread, and the IID is a compile-time constant
    // matching the interface the result is stored as.
    self.application_view_collection = self
      .service_provider
      .as_ref()
      .and_then(|provider: &IServiceProvider| unsafe {
        provider.QueryService(&IApplicationViewCollection::IID).ok()
      });

    // Re-create the taskbar list.
    // SAFETY: A `ComInit` only exists on a thread where `new` ran
    // `CoInitializeEx`, and the CLSID and the interface being requested
    // are compile-time constants that match each other.
    self.taskbar_list =
      unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_SERVER) }.ok();
  }

  /// Executes a COM operation, refreshing interfaces on failure and
  /// retrying once. Use this for operations that may fail due to stale
  /// COM interfaces.
  pub fn with_retry<T, F>(&mut self, op: F) -> crate::Result<T>
  where
    F: Fn(&Self) -> crate::Result<T>,
  {
    if let Ok(result) = op(self) {
      Ok(result)
    } else {
      self.refresh();
      op(self)
    }
  }
}

impl Default for ComInit {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for ComInit {
  fn drop(&mut self) {
    // Explicitly drop COM interfaces first.
    drop(self.taskbar_list.take());
    drop(self.application_view_collection.take());
    drop(self.service_provider.take());

    // SAFETY: This balances the `CoInitializeEx` that `new` ran on this
    // same thread, and every interface obtained from it was dropped
    // above.
    unsafe { CoUninitialize() };
  }
}

/// Undocumented COM interface for Windows shell functionality.
///
/// Note that filler methods are added to match the vtable layout.
///
/// # Safety
///
/// Every method dispatches blindly through the vtable of the object
/// behind `self`, so an instance may only come from the shell service
/// identified by the IID above, and may only be used while that object
/// is alive on the thread COM was initialized on. `m1` to `m3` stand in
/// for entries whose signatures are unknown, and must never be called.
#[windows_interface::interface("1841c6d7-4f9d-42c0-af41-8747538f10e5")]
pub unsafe trait IApplicationViewCollection: IUnknown {
  pub unsafe fn m1(&self);
  pub unsafe fn m2(&self);
  pub unsafe fn m3(&self);
  /// Writes the application view for a window handle to
  /// `application_view`.
  ///
  /// # Safety
  ///
  /// `application_view` must point at a writable
  /// `Option<IApplicationView>` that stays alive until the call returns.
  pub unsafe fn get_view_for_hwnd(
    &self,
    window: isize,
    application_view: *mut Option<IApplicationView>,
  ) -> HRESULT;
}

/// Undocumented COM interface for managing views in the Windows shell.
///
/// Note that filler methods are added to match the vtable layout.
///
/// # Safety
///
/// Every method dispatches blindly through the vtable of the object
/// behind `self`, so an instance may only come from
/// `IApplicationViewCollection`, and may only be used while that object
/// is alive on the thread COM was initialized on. `m1` to `m9` stand in
/// for entries whose signatures are unknown, and must never be called.
#[windows_interface::interface("372E1D3B-38D3-42E4-A15B-8AB2B178F513")]
pub unsafe trait IApplicationView: IUnknown {
  pub unsafe fn m1(&self);
  pub unsafe fn m2(&self);
  pub unsafe fn m3(&self);
  pub unsafe fn m4(&self);
  pub unsafe fn m5(&self);
  pub unsafe fn m6(&self);
  pub unsafe fn m7(&self);
  pub unsafe fn m8(&self);
  pub unsafe fn m9(&self);
  /// Cloaks or uncloaks the view.
  ///
  /// # Safety
  ///
  /// No pointers are passed, but both arguments go straight to the
  /// shell, which only defines behaviour for the documented cloak type
  /// and flag values.
  pub unsafe fn set_cloak(
    &self,
    cloak_type: u32,
    cloak_flag: i32,
  ) -> HRESULT;
}
