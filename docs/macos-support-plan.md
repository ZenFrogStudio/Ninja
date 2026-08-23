# macOS support plan

Ninja currently ships for Windows only. The codebase is *structured* for
macOS — the platform split is clean and nothing built so far forecloses it
— but **no part of it has ever been compiled for macOS**, let alone run.

This document records what's known to be missing, what's untested, and the
order to tackle it in. Written 2026-08-17, after the Windows build was
working.

## Prerequisites

None of this can be done from Windows:

- A Mac (Apple Silicon preferred; the CI matrix builds both `x86_64` and
  `aarch64` Apple targets)
- Xcode command line tools
- An Apple Developer account, for signing and notarisation. The existing
  CI workflow already expects `APPLE_CERTIFICATE`,
  `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
  `APPLE_ID_PASSWORD` and `APPLE_TEAM_ID`.

## 1. Get it to compile

Expect errors. The following was written against the macOS APIs but never
built:

- **`packages/wm-platform/src/platform_impl/macos/ipc.rs`** — the Unix
  domain socket transport that replaces the Windows named pipe. Roughly 80
  lines, written blind. Specific things to verify:
  - `UnixListener::bind` requires a Tokio runtime context. `IpcServer::start`
    should provide one, but this is reasoning rather than evidence.
  - The `DirBuilderExt` / `PermissionsExt` imports are `std::os::unix`-only.
  - Stale socket removal after a crash is implemented but unproven.

- **The vendored bar** (`bar/`) has macOS dependencies (`objc2-*`, `cocoa`)
  that upstream Zebar maintained, but our merged workspace pins different
  versions of shared crates than upstream did.

Run `cargo check --workspace` first and work through it before running
anything.

## 2. Known-wrong behaviour to fix

These compile, but are known to be incorrect on macOS:

- **`wm_platform::is_light_theme()` has no macOS implementation.** It
  returns `false` unconditionally. The tray icon is currently gated to
  always use the dark artwork on macOS as a stopgap
  (`packages/wm/src/sys_tray.rs`).

  The proper fix is twofold: implement detection via
  `NSApp.effectiveAppearance` compared against `NSAppearanceNameDarkAqua`,
  and — more idiomatically — supply the menu bar icon as a **template
  image**, which macOS inverts automatically. Check whether the `tray-icon`
  crate exposes template image support before writing appearance detection.

- **`bar/resources/starter/styles.css`** is hardcoded light-on-dark, and
  the bar logo is the white variant. Fine against a dark desktop, wrong
  against a light one. This is already true on Windows; macOS users are
  more likely to notice.

## 3. Things that have no macOS equivalent

Not bugs — they're correctly `cfg`-gated already, but they mean the macOS
build is a different product in these areas:

- **`wm-watcher`** is Windows-only. It exists because Windows hides windows
  by cloaking them, which survives the WM dying. macOS hidden windows stay
  accessible, so no watcher is needed.
- **`window_chrome.rs`** styles the settings window's title bar via DWM.
  macOS has no equivalent; the window will use the system default.
- **UIAccess** and `windows_subsystem` are Windows concepts.
- **The named pipe security descriptor.** On macOS the Unix socket relies
  on `0600` permissions in a `0700` directory instead. Equivalent in
  effect, entirely different mechanism.

## 4. Verify the transport end-to-end

The Windows pipe took several rounds of real debugging to get right. Give
the socket the same treatment rather than assuming it works because it
compiles:

1. `ninja-cli query monitors` returns JSON
2. The bar's Ninja provider connects — its startup logging reports
   `Ninja initial state: N monitors, M windows`
3. Workspaces render in the bar, and clicking one focuses it
   (`runCommand` through the provider-function path)
4. The settings window's WM page reads and writes the config
5. Two processes cannot interfere: confirm another user's account cannot
   reach the socket

## 5. Packaging

`resources/scripts/package.ps1` is PowerShell and WiX — **Windows only**.
There is no macOS equivalent in it.

The `.dmg` flow does exist in `.github/workflows/package.yaml`
(`package-macos`), covering universal binaries via `lipo`, `.icns`
generation from `resources/assets/icon.png`, code signing, DMG creation and
notarisation. It was written by upstream Zebar/GlazeWM and has not been run
since the merge.

Points to check:
- The workflow builds the WM and the CLI but **not the bar**.
  Bundling the bar into the macOS app was never part of upstream's flow and
  needs designing.
- Product naming — the Windows installers were renamed to Ninja with fresh
  UpgradeCodes; the macOS bundle identifier needs the same treatment.

## 6. Open design questions

- **Does the bar ship on macOS at all?** The `+` button, workspace
  switching and the settings GUI all assume it. If the answer is no, the
  macOS build is the WM alone and the settings GUI disappears with it —
  which would leave macOS with no way to edit gaps or borders except the
  config file.
- **Where does config live?** Currently `~/.ninja/config.yaml` on both
  platforms. macOS convention would be `~/Library/Application Support/`.
- **Keybindings.** The default config uses `alt` as the primary modifier
  and `ctrl` as the generated second modifier. On macOS `cmd` is the
  conventional primary, and `ctrl+alt` has no AltGr problem there — so the
  defaults likely want revisiting per platform.

## Recommended order

1. Compile (`cargo check --workspace`) and fix what breaks
2. Fix `is_light_theme` and the tray icon properly
3. Run the WM alone; verify the socket transport
4. Decide whether the bar ships on macOS
5. If yes: build and verify the bar, including settings
6. Packaging, signing, notarisation
7. Revisit default keybindings and config location

Steps 1–5 need a Mac in front of you. There is no way around that.
