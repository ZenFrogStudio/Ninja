<div align="center">
  <img src="./resources/assets/logo.svg" width="230" alt="Ninja logo" />

# Ninja

**A tiling window manager for Windows, with a built-in status bar.**

</div>

Ninja arranges your windows for you and gives you keyboard commands to move
between them. The status bar comes with it — workspaces, CPU, memory,
battery, media, the system tray — and runs in the same process as the
window manager, so there is one program to install and one to keep running.

Ninja is a fork of [GlazeWM](https://github.com/glzr-io/glazewm) with
[Zebar](https://github.com/glzr-io/zebar) vendored in as the bar. See
[NOTICE.md](./NOTICE.md).

## Install

Run `dist/ninja-setup.exe`. It installs to `C:\Program Files\Ninja`, adds
`ninja` and `ninja-cli` to your `PATH`, and installs the WebView2 runtime
if the machine doesn't already have it.

To uninstall, use Apps & features.

## Config

Your config lives at `~/.ninja/config.yaml`. It is created from the sample
in `resources/assets/sample-config.yaml` the first time Ninja starts.

| Path | What |
| --- | --- |
| `~/.ninja/config.yaml` | Keybindings, gaps, window rules, workspaces |
| `~/.ninja/workspaces.json` | Workspaces added at runtime, kept across restarts |
| `~/.ninja/errors.log.<date>` | Error log, rotated daily |
| `~/.ninja/bar/` | Bar settings and widget packs |

Point Ninja at a different config with `ninja start --config <path>`, or by
setting `NINJA_CONFIG_PATH`.

Upgrading from a build that used `~/.glzr/`? Ninja copies
`~/.glzr/glazewm` into `~/.ninja` and `~/.glzr/zebar` into `~/.ninja/bar`
on first run. The old folders are left alone.

## Build from source

Prerequisites:

- The Rust toolchain in `rust-toolchain.toml` (installed automatically by
  `rustup` on first build)
- [pnpm](https://pnpm.io) 9
- The [WiX](https://wixtoolset.org) CLI, with the `UI`, `Util` and
  `BootstrapperApplications` extensions
- The Windows SDK, for `signtool.exe`

Then, from an elevated PowerShell:

```powershell
pnpm --dir bar install
./resources/scripts/dev-cert.ps1          # once, for local signing
./resources/scripts/package.ps1 -VersionNumber 0.3.0 -DevSign
```

That produces `out/ninja-setup.exe` along with the three executables in
`out/x64`.

Signing matters for more than trust here: Windows only grants UIAccess to
signed binaries, and without it Ninja cannot move or resize windows owned
by elevated processes. `package.ps1` compiles the feature out when it has
nothing to sign with, so an unsigned build still runs. See
[docs/signing.md](./docs/signing.md).

## Develop

`./run.ps1` builds and starts Ninja against `.testconfig/config.yaml` if
present, otherwise your own config. `./run.ps1 -Stop` stops it.

## Layout

| Crate | What |
| --- | --- |
| `ninja` (`bar/packages/desktop`) | The application. Hosts the bar and the window manager in one process, and owns the Tauri build. Produces `ninja.exe`. |
| `wm` | Core window management logic. Library only. |
| `wm-cli` | The `ninja-cli` command. |
| `wm-common` | Shared types, and the `~/.ninja` path helpers. |
| `wm-ipc-client` | Client for the WM's IPC endpoint. |
| `wm-platform` | Windows and macOS API wrappers. Nothing else calls the OS directly. |
| `wm-watcher` | Watchdog that restores hidden windows if Ninja crashes. |

The bar's frontend lives in `bar/packages/client-api` (the widget client
API) and `bar/packages/settings-ui` (the settings window). Both are built
before the Rust build, which embeds them.

## Widgets

Widgets are plain HTML that import the client API Ninja serves:

```js
import * as ninja from '/__ninja/client.js';
```

The starter pack in `bar/resources/starter` is installed on first run and
is the best place to start reading. Widget packs written against upstream
Zebar keep working: the `/__zebar/*` routes, `window.__ZEBAR_STATE`, and
the `glazewm` provider type are all still accepted.

## Licence

GPL-3.0-only. See [LICENSE.md](./LICENSE.md).
