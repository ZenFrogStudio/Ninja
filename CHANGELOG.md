# Changelog

## Unreleased

### Changed

- The tray's Settings, Widgets and Reload bar items now talk to the bar
  directly instead of launching a second `ninja.exe` to forward the
  request.
- Updated `sysinfo`, removing one duplicate copy of the `windows` crate
  from the build.
- CI now runs the Rust test suite, and clippy warnings in the bar fail
  the build like they do for the window manager.
- The client API now has a vitest test suite, run in CI.
- Every `unsafe` block in the workspace now carries a `SAFETY` comment,
  and clippy enforces it.
- `run.ps1` builds and launches the single `ninja.exe` instead of the
  separate bar and window manager binaries it no longer produces.

### Security

- Widgets can no longer run `shell-exec` through the `ninja` provider's
  `runCommand`. Use `shellExec` with a `shellCommands` privilege.
- Shell privileges now resolve the program to an absolute path with the
  bar's own `PATH` before checking, and a widget-supplied `PATH` is
  ignored.
- **Breaking:** a shell privilege's `argsRegex` must now match the whole
  argument string. Patterns that relied on matching a substring need
  anchoring removed, e.g. `status` becomes `.*status.*`.

### Fixed

- Processes started with `shellSpawn` are forgotten when they exit
  instead of being held until the bar closes.
- A fatal error while the bar starts now shows an error dialog instead of
  exiting silently.
- "Run on system startup" on Windows now checks that the startup entry
  points at the running executable, and writes the path quoted. An entry
  left behind by another copy of Ninja (a dev build, or an install that
  has moved) showed the option as on while starting the wrong binary.
- A widget command the window manager refuses is now logged with its
  error instead of failing silently.
- Installing or upgrading no longer deletes the "Run on system startup"
  entry. The installer removed it on every install, not just on
  uninstall, so Ninja stopped starting with Windows after each upgrade.

### Removed

- The unused `start_preview_widget` and `stop_all_preview_widgets` commands.
- The bar's unused tray menu code. The window manager's tray is the only
  one.

## 0.3.3 - 2026-09-06

### Security

- A widget can no longer write to or kill a process spawned by another
  widget. `shellWrite` and `shellKill` now check that the calling widget
  owns the process and reject anything else.

### Fixed

- Quoted shell-exec commands (for example `"C:\Program Files\App\app.exe"
  --flag`) failed with "command doesn't have an ending quote". The parser
  now stops at the closing quote.
- Widget pack and widget names are validated before they are used as
  directory names, and the create-pack dialog shows a field error for an
  invalid name instead of a raw error string.
- Deleting a widget now removes its files from disk, not just the config
  entry.
- The client API types now match what the bar returns: the `ninja` provider
  function and systray double-click are typed, `runCommand` returns the
  documented `{ subjectContainerId }` object, and the shell `onExit` status
  is `{ code, success, signal }` rather than `{ exitCode, signal }`.
- The bar no longer logs every command's arguments and response to the
  widget console. Only the command name is logged.

### Changed

- Client API package `ninja` bumped to 3.1.0.
- `pnpm run typecheck` runs `tsc --noEmit` for `client-api` and
  `settings-ui`. Prettier now ignores build output and generated schemas.

## 0.3.2 - 2026-09-06

### Fixed

- Window drops at a shared monitor edge now resolve to exactly one monitor,
  preventing intermittent placement on the adjacent display.
- Window redraw events initiated by Ninja no longer feed back into drag and
  workspace state while another window is being repositioned.
- Tiling resize proportions can no longer become non-finite when every sibling
  is at its minimum size. Existing invalid proportions are repaired on the next
  resize instead of leaving windows at a fraction of their allotted area.
- Windows dragged from a tiled layout are classified at drag start and remain
  tiled when Windows Aero Snap maximizes them before the drop event arrives.

## 0.3.1 — 2026-08-30

### Fixed

- The WM no longer freezes when moving focus onto a workspace whose most
  recently focused window is floating, minimized, or fullscreen. The
  iterator over a container's focus order never advanced — it re-scanned
  from the start on every step, so it returned the same child forever and
  never ended. Searching it for a tiling child therefore spun on the spot.
  Because the WM runs its event loop on one thread, that one search took
  everything down with it: keybindings stopped responding, the bar stopped
  updating, and the redraw that hides the windows of workspaces you aren't
  looking at never ran, leaving every window from every workspace on screen
  at once.
- Workspaces added with the bar's **+** button now come back to the screen
  they were added on. They were recorded against the monitor's *position*,
  which the WM recomputes from screen coordinates on every display change,
  so a resolution change, a monitor waking up, or rearranging the displays
  could send the workspace to a different screen. They're now recorded
  against the display itself — its device path on Windows, its UUID on
  macOS — which is the same identifier the WM already uses to recognise a
  monitor across a disconnect.

  Existing `workspaces.json` files keep working: an entry written before
  this change still binds by index until the workspace is added again.
- Workspaces displaced by a monitor disconnect now return to that monitor
  when it comes back. Tearing a monitor down moves its workspaces onto a
  surviving monitor so their windows aren't lost, but only workspaces bound
  via `bind_to_monitor` were ever moved back — the rest stayed piled up on
  one screen. This is most visible on Windows, which can reissue a
  monitor's handle across a display wake and make a panel that never went
  away look like a new one. Each displaced workspace now remembers where it
  came from until it gets back, and a workspace you move yourself stays
  where you put it.
- The bar now starts on installs carried over from GlazeWM and Zebar.
  Startup settings written before the fork ask for the `glzr-io.starter`
  pack and its `with-glazewm` widget, both of which were renamed, so
  nothing matched and the bar came up empty. Those names are now migrated
  to `ninja.starter` and `with-ninja`.

  The starter pack is also reinstalled whenever its files are missing
  rather than only on a first run. Packs are stored under the application
  name, so the rename left the previous downloads stranded while the
  config directory — which records what is installed — carried over intact.

## 0.3.0 — 2026-08-23

### Changed

- The project was moved to a clean tree and renamed throughout. The crate
  that builds `ninja.exe` is now `ninja` rather than `zebar`; the npm
  packages are `ninja`, `@ninja/settings-ui` and `@ninja/desktop`; the
  bar's window-manager provider is `ninja` rather than `glazewm`. The
  `wm-*` crates keep their names — "wm" is short for window manager, not
  leftover branding.
- Product metadata is now Ninja's: installer manufacturer, Tauri bundle
  identifier (`com.ninja.app`), executable file descriptions, and the
  macOS `Info.plist`. Installer `UpgradeCode`s are unchanged, so upgrades
  over an existing install still work.
- User config and data moved from `~/.glzr/glazewm/` and `~/.glzr/zebar/`
  to `~/.ninja/` and `~/.ninja/bar/`. On first run Ninja copies the old
  directories across if the new one doesn't exist yet; the originals are
  left in place. `NINJA_CONFIG_PATH` is the config override env var, with
  `GLAZEWM_CONFIG_PATH` still read as a fallback.
- The IPC pipe is now `\\.\pipe\ninja-ipc-<user>` (macOS:
  `~/.ninja/ipc.sock`), and the internal dispatch window message is
  `Ninja:Dispatch`.
- The sample config no longer launches or kills a separate bar process —
  the bar has run in the WM's process since 0.2.0. Its ignore rule now
  matches `ninja`.
- Widgets import the client API from `/__ninja/client.js`, and the widget
  state is injected as `window.__NINJA_STATE`.

### Compatibility

Widget packs written against upstream Zebar still load. The `/__zebar/*`
asset routes, the `ZEBAR_TOKEN` cookie, `window.__ZEBAR_STATE` and the
`glazewm` provider type are all kept as aliases of their Ninja names.

## 0.2.0 — 2026-08-17

### Added

- `add-workspace` command, for adding a workspace that persists across
  restarts. Takes an optional `--name` and otherwise uses the lowest unused
  positive integer. The workspace is bound to the monitor of the subject
  container, so `--id <monitor-id>` decides which monitor it lands on.
- Added workspaces are recorded in `workspaces.json`, alongside the config
  file, and merged into the workspace list on load. They're held separately
  so the WM never rewrites the user's config, which would discard its
  comments and formatting. Anything declared in the config wins over the
  store, and a missing or malformed store is ignored rather than being
  fatal. A config passed via `--config` gets its own store.

### Fixed

- The bar's "add workspace" (+) button now adds a workspace on every click,
  and the workspaces it adds survive a restart. It used to create the
  workspace by focusing a name that wasn't in use, but such a workspace has
  no config entry and so isn't `keep_alive` — the WM tore it down as soon
  as focus left it while empty. The second click therefore appeared to
  renumber the workspace the first click added instead of adding another.
- Workspaces bound to a monitor with `bind_to_monitor` no longer vanish when
  that monitor is disconnected and reconnected. Tearing a monitor down
  discarded every workspace on it that was empty and not `keep_alive`, and
  reattaching only recreated the `keep_alive` ones. Every bound workspace is
  now recreated on attach.

  **Behaviour change:** a workspace with `bind_to_monitor` set is now always
  active on its monitor, whether or not `keep_alive` is set. Configs that
  relied on bound workspaces being torn down once empty will see a higher
  workspace count at startup; set `bind_to_monitor` only on workspaces you
  want permanently present.
- A single window without a workspace no longer aborts the whole
  display-settings handler. The DPI fixup loop bailed on the first such
  window, skipping the remaining windows and the full-tree redraw that
  follows it, which left the container tree half-synced.
- Display changes are now ignored while the displays are powered off. The
  WM had no way to tell an idle-timeout blank apart from the user actually
  unplugging their monitors: on many GPUs the blank drops the video link,
  every monitor disappears at once, and the WM tore its monitor layout down
  in response. It now registers for `GUID_CONSOLE_DISPLAY_STATE`
  notifications and holds off until the panels are back, then re-syncs
  once — messages dropped while the displays were off are never replayed
  by Windows.
- The suspend latch no longer sticks on. `PBT_APMRESUMECRITICAL`, sent when
  the machine comes back from an unexpected power loss, wasn't recognised
  as a resume, so display changes stayed ignored for the rest of the
  process' life. Every resume variant now clears the flag.
- A transient failure to reapply mouse config no longer exits the WM. The
  call sat inside a `tokio::select!` branch, where its `?` propagated out
  of the main loop entirely and turned a recoverable error into a fatal
  one. It's now logged like every other error in that loop.
- The bar no longer closes all of its widgets when the system reports zero
  monitors. It rebuilt its widget layout on any monitor change, which with
  no displays attached meant closing every widget and opening none, leaving
  nothing to restore when the panels woke up.
- The bar's workspace indicators no longer disappear for good. Its
  connection to the WM was made exactly once and never retried, so losing
  the race to the WM's IPC listener at startup — a matter of a hundred
  milliseconds, and the usual outcome when both are launched together —
  left the workspace widget blank until the bar was restarted. It now
  reconnects with a capped backoff, for the life of the provider, and a
  dropped connection is picked up within seconds rather than parking the
  provider silently.
- A stopped provider in the bar can be started again. Providers are shared
  between widget windows and keyed by their config, and a provider that had
  ended stayed registered forever: every widget window opened afterwards
  replayed its final error instead of getting a live one. Widget windows
  are rebuilt on every display change, so one failure at startup propagated
  to every window for the rest of the session.
- The WM no longer drops a client's event subscription when its outbound
  queue briefly fills. A full queue ended the subscription outright while
  leaving the connection open, so the client was never told and simply
  stopped receiving events. Since the queue fills during bursts — a display
  wake fires an event per window — this hit precisely when the bar most
  needed to resync. Events are now shed individually; only a closed
  connection ends the subscription.
- The bar's workspace widget no longer breaks when the WM reports no
  monitors, or when Tauri reports no current monitor for the widget's
  window. Both are normal while displays power down and come back, and both
  threw inside the emission handler, which stopped the widget updating for
  the rest of the window's life.
- Windows are no longer lost when the WM restarts. With
  `hide_method: 'cloak'`, windows on non-displayed workspaces are hidden by
  cloaking them, but neither exit path ever uncloaked them: both called
  `show`, which reverses the `SW_HIDE` used by `hide_method: 'hide'` and
  does nothing to a cloak. A cloaked window is absent from the taskbar and
  the task switcher alike, and the next session couldn't reclaim it either,
  since a cloaked window reports itself as not visible — so whatever sat on
  a hidden workspace at exit was gone for good, still running and
  unreachable. Both the WM and the watchdog now uncloak what they restore.

  Restoration also happens before the WM announces its exit rather than
  after. The watchdog stands down as soon as it sees `ApplicationExiting`,
  so anything done after that point had no safety net.

  Windows stranded by an earlier session are reclaimed at startup, which
  recovers the ones already lost and covers the exits that never get to
  run at all, such as a power loss.
- The watchdog's cleanup budget is now a real bound. Restoration runs on
  its own thread, so the five seconds it is allowed actually expire: most
  of the work goes through COM, which blocks with no timeout when a
  window's owning process is unresponsive, and the deadline could only be
  checked between windows, never during a call already in progress. A
  watchdog that wedged there outlived the WM indefinitely and left the next
  session with no safety net.
- The cursor no longer jumps to another monitor when an app is clicked from
  the Windows taskbar. With `cursor_jump.trigger` set to `monitor_focus`,
  the pointer was thrown to the middle of another monitor mid-click,
  usually on the second click of a taskbar button.

  Two paths queued the jump while merely reacting to the OS, rather than
  carrying out something the user asked for. The second click on a taskbar
  button minimizes the window, and the minimize handler reassigned focus to
  whatever window was next — often on another monitor — and jumped the
  cursor after it. Clicking a button whose window sits on a hidden
  workspace also switches to that workspace, and the workspace switch
  carried its own jump.

  Cursor jumps now belong to focus *commands*. A keyboard-driven focus
  change still brings the cursor along, so it isn't stranded on the monitor
  it came from; a focus change the user made with the mouse leaves it where
  they put it.

  **Behaviour change:** minimizing a window no longer moves the cursor,
  whether by mouse or by the `toggle-minimized` keybinding. Focus landing
  elsewhere after a window disappears is fallout from the window going
  away, not a focus change aimed at a monitor.
- Monitors are matched to displays by device path first, then hardware ID,
  then handle — and each identifier is now tried against every monitor
  before falling back to a weaker one. Windows recycles monitor handles
  across a display wake, so a handle could be reissued from one panel to
  another; matching on it first let a monitor claim the wrong display and
  take its workspaces to the wrong screen.

### Diagnostics

- Panics are written to the log file. Release builds run without a console,
  so the default panic message went to a stderr that goes nowhere, and a
  panic was indistinguishable from the process simply vanishing. Both the
  WM and the bar now log the payload, location, thread and backtrace.
- A panic in the WM no longer leaves the process wedged. Stopping the event
  loop happened inline on the success path only, so an unwinding WM thread
  left the main thread blocked in the event loop forever. It's now stopped
  from a guard that runs either way, and the panic is reported rather than
  raised a second time on the main thread.
- Log files rotate daily instead of growing without bound in a single
  `errors.log`, so an overnight incident lands in a dated file of its own.
  Startup lines carry the process ID, so overlapping sessions in one file
  can be told apart.
- The one place that posts `WM_QUIT` now logs it. If the event loop ever
  returns without that line, the quit came from outside our own code.
- Display power state, suspend and resume broadcasts, and display messages
  dropped while the panels were off are logged. The WM has to react to all
  of these, and without them a mishandled wake is invisible after the fact.

  The verbose per-display dump that went alongside them — every reported
  display and stored monitor with its handle, device path, hardware ID and
  bounds, and which identifier each match came from — has been removed now
  that it has served its purpose. It found the recycled monitor handle
  behind workspaces moving to the wrong screen after a wake.
- IPC messages are logged at debug rather than info. The bar re-queries the
  full WM state on every event, which is several messages apiece and buried
  everything else in the log.

### Changed

- The bar and the window manager are one program. `ninja.exe` now hosts
  both, and `ninja-bar.exe` is gone.

  The bar reaches the WM through an in-process channel rather than a
  socket. It sends the same command strings to the same handlers — only
  the transport is gone — which removes the failure mode behind this
  release's worst bug: a connection that could be lost, and once lost,
  never recovered. There is nothing left to connect to, or to race at
  startup.

  Layout is one thread each: Tauri keeps the main thread, the WM's platform
  event loop gets its own, and the WM runs on a third. The event loop can't
  share Tauri's thread — its low-level keyboard hook is called there, and
  Windows silently unregisters a hook whose callback overruns, which a slow
  webview frame could easily cause. The WM needs a thread rather than a
  task because its container tree is built on `Rc<RefCell<..>>`.

  Shutting down either half now takes the other with it. The WM restores
  its windows first, then the app exits, so quitting can't strand windows.

  **Behaviour changes:**
  - One tray icon instead of two. The window manager's menu is the one that
    remains; the bar's widget-pack shortcuts are still reachable from the
    settings window it opens.
  - One log file. Both halves write to `~/.glzr/zebar/errors.log.<date>`,
    since a process can only install one subscriber.
  - Setup installs a single MSI. The separate bar package is gone.

  **Windows only.** The merged startup is not yet gated by platform, and
  macOS cannot work as written: both tao and `wm-platform` want
  `NSApplication` on the main thread, and the WM's event loop is started on
  a thread of its own. That is a real conflict rather than an effort
  problem, and the macOS path needs gating back to two processes before it
  can build meaningfully.

### Internal

- `wm` is a library with a thin binary on top, so the window manager can be
  hosted by another process instead of owning one.
- `wm-platform` moved from the `windows` crate 0.52 to 0.58, the version
  the vendored bar already used. The two were semver-incompatible and were
  being built side by side, which meant their `HWND` and friends were
  distinct types that couldn't be passed between the two halves of the
  application.

  Handle types wrap a raw pointer rather than an `isize` as of 0.58, so
  they're no longer `Send`. Hook handles that cross the thread they're
  registered on — `HHOOK` in the keyboard hook, `HWINEVENTHOOK` in the
  window listener — are now carried as `isize` and rebuilt on the far side,
  matching how window and monitor handles were already stored.

  `DisplayExtWindows::hmonitor` returns an `isize` instead of an
  `HMONITOR`, keeping the `windows` crate's types inside `wm-platform`.

## 0.1.0 — 2026-08-16

First Windows release of the Ninja fork.

### Installer

- `ninja-setup.exe` is the single download. It chains the window manager
  and bar MSIs, and is produced from whichever Rust targets are installed
  on the build machine rather than requiring both x64 and arm64. A machine
  set up for x64 only now gets a working setup instead of none.
- The window manager and the bar install into one directory,
  `C:\Program Files\Ninja`. They were previously split across
  `Ninja\wm` and `glzr.io\Ninja Bar`, which broke the tray's "Bar
  settings" and "Reload bar" items on a real install — the WM resolves the
  bar binary as a sibling of its own executable.
- Setup UI no longer offers to install "GlazeWM" and "Zebar", and no longer
  links to the upstream repositories.
- Fixed the x64 bar package ignoring the "install the bar" checkbox. The
  install condition was missing parentheses, so `AND` bound tighter than
  `OR` and the package installed regardless.
- Fixed the setup crashing during an interactive install. The chained
  MSIs drew their own UI on top of the bundle's; with Ninja Bar running,
  Restart Manager tried to raise a FilesInUse prompt through it, which
  produced Windows Installer error 2884 and then heap corruption in the
  Burn engine (`0xc0000374`). The bundle's theme is now the only UI and
  the MSIs run silently beneath it
  ([wix#8695](https://github.com/wixtoolset/issues/issues/8695)).
- Moved the desktop and Start Menu shortcut choices onto the setup's own
  welcome page. They were in the WM MSI's dialogs, which no longer
  display; `bundle.wxs` forwards them as `ENABLE_DESKTOP_SHORTCUT` and
  `ENABLE_START_MENU_SHORTCUT`.
- `out/` now contains only the three shippable artifacts. The detached
  Burn engine and the pre-reattach bundle looked interchangeable with the
  real setup and invited running the wrong one.
- Fixed `ninja-setup.exe /quiet` and `/passive` hanging forever. Both
  chained MSIs had `DisplayInternalUICondition="1"`, which is always true,
  so they forced their own UI up and waited on a dialog nobody could
  click. The condition is now gated on `WixBundleUILevel`, so an
  interactive install still gets the MSI's own options.
- Dropped the `ADD_GLAZEWM_STARTER` property from the chain. Nothing in the
  vendored bar installer consumes it.
- Removed a doubled separator in the PATH entry for the CLI directory.

### Signing

- `ui_access` is now enabled only when the build can actually sign. It was
  unconditional, which produced installers whose app refused to start:
  Windows grants UIAccess solely to signed binaries, so an unsigned build
  failed at launch with "A referral was returned from the server". CI
  builds with the `AZ_*` secrets set are unaffected and still get
  UIAccess.
- Added `resources/scripts/dev-cert.ps1` and `package.ps1 -DevSign` for a
  locally-trusted self-signed certificate, so a local build can carry
  UIAccess. Refuses to run under CI.
- The WM logs whether UIAccess is active at startup. Without it, windows
  owned by elevated processes cannot be moved, and the only prior symptom
  was a repeated "Failed to set window position" warning.
- See `docs/signing.md`.

### Renamed

Artifacts users see are now Ninja throughout. Config paths
(`~/.glzr/glazewm/`), the IPC pipe name, and internal crate and npm package
names are unchanged.

| Was | Now |
|---|---|
| `glazewm.exe` | `ninja.exe` |
| `glazewm-cli.exe` | `ninja-cli.exe` (installed as `cli\ninja.exe`) |
| `glazewm-watcher.exe` | `ninja-watcher.exe` |
| `zebar.exe` | `ninja-bar.exe` |
| `installer-x64.msi` | `ninja-wm-x64.msi` |
| `zebar-x64.msi` | `ninja-bar-x64.msi` |
| `installer-universal.exe` | `ninja-setup.exe` |
| `com.glzr.zebar` | `com.glzr.ninja` |
| `with-glazewm` starter widget | `with-ninja` |

The bar picks its default starter widget by probing PATH for the window
manager, which was still looking for `glazewm`; it now looks for `ninja`.

The bar also still introduced itself as `zebar` in `--version` and
`--help`, since clap falls back to the crate name. It reports `ninja-bar`
now.

Two config references were missed in the first pass and broke startup:

- `startup_commands` still ran `shell-exec zebar`, so the bar never
  launched with the WM and the log filled with "Shell exec failed for
  'zebar'".
- The `window_process: { equals: 'zebar' }` ignore rule no longer matched,
  so the WM treated its own bar as a tileable window.

Both are fixed in `resources/assets/sample-config.yaml` and
`.testconfig/config.yaml`. **Existing configs need the same two edits** —
`~/.glzr/glazewm/config.yaml` is not migrated automatically.

Not renamed: the bar still keeps its settings in `~/.glzr/zebar/`.
Changing that needs a migration for existing installs.

### Fixed

- A widget could set `refreshInterval: 0` and hang the app. The catch-up
  loop in `SyncInterval::tick` never advanced, permanently burning a core
  and a blocking-pool thread, and the network provider's per-second
  calculation divided by zero. Refresh intervals are now clamped to 100 ms.

