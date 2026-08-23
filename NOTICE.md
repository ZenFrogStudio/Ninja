# Notice

Ninja is a derivative work of two projects by Glzr Software Pte. Ltd., both
licensed under the GNU General Public License v3.0:

- **GlazeWM** — https://github.com/glzr-io/glazewm
  Ninja's window management core (`packages/wm`, `packages/wm-common`,
  `packages/wm-platform`, `packages/wm-cli`, `packages/wm-ipc-client`,
  `packages/wm-watcher`) is derived from it.

- **Zebar** — https://github.com/glzr-io/zebar
  Ninja's bar (`bar/`) is a vendored fork of it.

Ninja has been modified from both: the bar and the window manager were
merged into a single process, the projects were renamed, and user config
was moved from `~/.glzr/` to `~/.ninja/`.

Ninja is licensed under the GPL-3.0-only, the same terms as its upstreams.
The full licence text is in [LICENSE.md](./LICENSE.md).

Ninja is not affiliated with or endorsed by Glzr Software Pte. Ltd.

## Third-party dependencies

Ninja links against a number of third-party crates and npm packages, each
under its own licence. Two are worth naming because they carry upstream
names in this tree:

- `glazewm` (npm) — TypeScript type definitions for the window manager's
  state, used by the bar's client API.
- `@glzr/components`, `@glzr/style-guide` (npm) — UI components and lint
  config used by the settings window.
