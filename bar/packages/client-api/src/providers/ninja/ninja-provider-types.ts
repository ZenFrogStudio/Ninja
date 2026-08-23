// Types only. The WM connection lives in the desktop process, so none of
// the `glazewm` package's websocket client is pulled into the bundle.
import type {
  TilingDirection,
  BindingModeConfig,
  Container,
  Monitor,
  RunCommandResponse,
  Workspace,
  Window,
} from 'glazewm';

import type { Provider } from '../create-base-provider';

export interface NinjaProviderConfig {
  type: 'ninja' | 'glazewm';
}

export type NinjaProvider = Provider<NinjaProviderConfig, NinjaOutput>;

export interface NinjaOutput {
  /**
   * Workspace displayed on the current monitor.
   */
  displayedWorkspace: Workspace;

  /**
   * Workspace that currently has focus (on any monitor).
   */
  focusedWorkspace: Workspace;

  /**
   * Workspaces on the current monitor.
   */
  currentWorkspaces: Workspace[];

  /**
   * Workspaces across all monitors.
   */
  allWorkspaces: Workspace[];

  /**
   * All monitors.
   */
  allMonitors: Monitor[];

  /**
   * All windows.
   */
  allWindows: Window[];

  /**
   * Monitor that currently has focus.
   */
  focusedMonitor: Monitor;

  /**
   * Monitor that is nearest to this Ninja widget.
   */
  currentMonitor: Monitor;

  /**
   * Container that currently has focus (on any monitor).
   */
  focusedContainer: Container;

  /**
   * Tiling direction of the focused container.
   */
  tilingDirection: TilingDirection;

  /**
   * Active binding modes;
   */
  bindingModes: BindingModeConfig[];

  /**
   * Whether Ninja is currently paused.
   */
  isPaused: boolean;

  /**
   * Invokes a WM command (e.g. `"focus --workspace 1"`).
   *
   * @param command WM command to run (e.g. `"focus --workspace 1"`).
   * @param subjectContainerId (optional) ID of container to use as subject.
   * If not provided, this defaults to the currently focused container.
   * @throws If command fails.
   */
  runCommand(
    command: string,
    subjectContainerId?: string,
  ): Promise<RunCommandResponse>;
}
