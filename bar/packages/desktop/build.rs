fn main() {
  // Declaring the app's commands generates an `allow-<command>`
  // permission for each, which capabilities can then reference.
  //
  // This is required because widget windows are served from the local
  // asset server, which Tauri treats as a *remote* origin. Since 2.11,
  // remote origins cannot reach app commands unless a capability grants
  // them explicitly.
  let app_manifest = tauri_build::AppManifest::new().commands(&[
    "widget_packs",
    "widget_states",
    "start_widget",
    "start_widget_preset",
    "stop_widget_preset",
    "update_widget_config",
    "create_widget_pack",
    "update_widget_pack",
    "delete_widget_pack",
    "create_widget_config",
    "delete_widget_config",
    "listen_provider",
    "unlisten_provider",
    "call_provider_function",
    "set_always_on_top",
    "set_skip_taskbar",
    "shell_exec",
    "shell_spawn",
    "shell_write",
    "shell_kill",
    "read_wm_settings",
    "write_wm_settings",
  ]);

  // When the `ui_access` feature is enabled, the `uiAccess` attribute is
  // set to `true`. UIAccess is disabled by default because it requires the
  // application to be signed and installed in a secure location.
  //
  // Both requirements are enforced by Windows at process start, and the
  // signature check cannot be waived: it runs "regardless of the state of"
  // the secure-locations policy, and there is no end-user override. An
  // unsigned build with `uiAccess="true"` therefore fails to launch at
  // all, with "A referral was returned from the server" — which is why
  // `package.ps1` only enables this feature when it can sign. See
  // `docs/signing.md`.
  let ui_access = {
    #[cfg(feature = "ui_access")]
    {
      "true"
    }
    #[cfg(not(feature = "ui_access"))]
    {
      "false"
    }
  };

  // Conditionally enable UIAccess, which grants privilege to set the
  // foreground window and to set the position of elevated windows.
  //
  // In practice only the second matters here: `NativeWindow::focus`
  // already works around the foreground lock by sending an input to its
  // own process first, which needs no privilege.
  //
  // Ref: https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/security-policy-settings/user-account-control-only-elevate-uiaccess-applications-that-are-installed-in-secure-locations
  //
  // Additionally, declare support for per-monitor DPI awareness. The
  // window manager needs both, and it now shares this binary with the bar.
  // The `Common-Controls` dependency comes from Tauri's own default
  // manifest, which passing one of our own replaces wholesale. Dropping it
  // breaks the side-by-side activation context, and the process then fails
  // to load before `main` ever runs — no output, no log, exit code 127.
  let app_manifest_xml = format!(
    r#"
<assembly
  xmlns="urn:schemas-microsoft-com:asm.v1"
  manifestVersion="1.0"
  xmlns:asmv3="urn:schemas-microsoft-com:asm.v3"
>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>

  <asmv3:trustInfo>
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="{ui_access}" />
      </requestedPrivileges>
    </security>
  </asmv3:trustInfo>

  <asmv3:application>
    <windowsSettings
      xmlns:ws2005="http://schemas.microsoft.com/SMI/2005/WindowsSettings"
      xmlns:ws2016="http://schemas.microsoft.com/SMI/2016/WindowsSettings"
    >
      <ws2005:dpiAware>true</ws2005:dpiAware>
      <ws2016:dpiAwareness>PerMonitorV2</ws2016:dpiAwareness>
    </windowsSettings>
  </asmv3:application>
</assembly>
"#
  );

  tauri_build::try_build(
    tauri_build::Attributes::new()
      .app_manifest(app_manifest)
      .windows_attributes(
        tauri_build::WindowsAttributes::new()
          .app_manifest(app_manifest_xml),
      ),
  )
  .expect("failed to run tauri-build");
}
