use std::ffi::c_void;

use anyhow::Context;
#[cfg(target_os = "windows")]
use windows::Win32::{
  Foundation::{HANDLE, INVALID_HANDLE_VALUE, WIN32_ERROR},
  NetworkManagement::WiFi::{
    wlan_intf_opcode_current_connection, WlanCloseHandle,
    WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle,
    WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES,
  },
};

#[derive(Debug)]
pub struct WifiHotstop {
  pub ssid: Option<String>,
  pub signal_strength: Option<u32>,
}

#[derive(Debug)]
#[cfg(target_os = "windows")]
struct WlanHandle(HANDLE);

#[cfg(target_os = "windows")]
impl Drop for WlanHandle {
  fn drop(&mut self) {
    // SAFETY: The handle is owned by this guard, which is the only thing
    // that closes it, so this runs exactly once. `WlanCloseHandle`
    // tolerates `INVALID_HANDLE_VALUE`, which is what the guard holds if
    // `WlanOpenHandle` never succeeded.
    unsafe {
      WlanCloseHandle(self.0, None);
    }
  }
}

/// Gets wifi ssid and signal strength using winapi
pub fn default_gateway_wifi() -> anyhow::Result<WifiHotstop> {
  #[cfg(not(target_os = "windows"))]
  {
    Ok(WifiHotstop {
      ssid: None,
      signal_strength: None,
    })
  }
  #[cfg(target_os = "windows")]
  {
    let mut pdw_negotiated_version = 0;
    let mut wlan_handle = WlanHandle(INVALID_HANDLE_VALUE);

    // SAFETY: Both out-parameters are owned by this frame and outlive the
    // call. Writing the handle straight into the guard means it gets
    // closed even if a later `?` returns early.
    WIN32_ERROR(unsafe {
      WlanOpenHandle(
        2,
        None,
        &mut pdw_negotiated_version,
        &mut wlan_handle.0,
      )
    })
    .ok()
    .context("Failed to open Wlan handle")?;

    let mut wlan_interface_info_list = std::ptr::null_mut();

    // SAFETY: The handle was opened successfully above. On success the
    // WLAN service allocates the interface list and hands us ownership,
    // which is released by the `WlanFreeMemory` below.
    WIN32_ERROR(unsafe {
      WlanEnumInterfaces(
        wlan_handle.0,
        None,
        &mut wlan_interface_info_list,
      )
    })
    .ok()
    .context("Failed to get Wlan interfaces")?;

    // SAFETY: The call above succeeded, so the pointer is non-null and
    // points at a `WLAN_INTERFACE_INFO_LIST` whose `InterfaceInfo` array
    // has at least one entry. The `GUID` is copied out before the
    // allocation is freed.
    let guid = (unsafe { *wlan_interface_info_list }).InterfaceInfo[0]
      .InterfaceGuid;

    // SAFETY: The list was allocated by `WlanEnumInterfaces` above, is no
    // longer read after the copy, and is freed exactly once.
    unsafe { WlanFreeMemory(wlan_interface_info_list as *mut c_void) };

    let mut data_size = 0;
    let mut pdata = std::ptr::null_mut();

    // SAFETY: The handle is still open, `guid` names an interface the
    // service just reported, and both out-parameters are owned by this
    // frame. On success the service allocates the result buffer and
    // hands us ownership, released by the `WlanFreeMemory` below.
    WIN32_ERROR(unsafe {
      WlanQueryInterface(
        wlan_handle.0,
        &guid,
        wlan_intf_opcode_current_connection,
        None,
        &mut data_size,
        &mut pdata,
        None,
      )
    })
    .ok()
    .context("Failed to get connected Wlan interface")?;

    let wlan_connection_atributes =
      pdata as *mut WLAN_CONNECTION_ATTRIBUTES;

    // SAFETY: `wlan_intf_opcode_current_connection` is documented to
    // return a `WLAN_CONNECTION_ATTRIBUTES`, and the query succeeded, so
    // the pointer is non-null and correctly typed. The attributes are
    // copied out before the allocation is freed.
    let atributes =
      unsafe { *wlan_connection_atributes }.wlanAssociationAttributes;

    // SAFETY: `pdata` was allocated by `WlanQueryInterface` above, is no
    // longer read after the copy, and is freed exactly once.
    unsafe { WlanFreeMemory(pdata) };

    // needed to remove leading zeros in array
    let ssid_arr = atributes.dot11Ssid.ucSSID;
    let mut ssid_vec = ssid_arr
      .into_iter()
      .rev()
      .skip_while(|&byte| byte == 0)
      .collect::<Vec<_>>();
    ssid_vec.reverse();
    let ssid =
      String::from_utf8(ssid_vec).context("Incorrectly formatted ssid")?;

    Ok(WifiHotstop {
      ssid: Some(ssid),
      signal_strength: Some(atributes.wlanSignalQuality),
    })
  }
}
