import type {
  Container,
  Monitor,
  RunCommandResponse,
  TilingDirection,
  Window,
  BindingModeConfig,
} from 'glazewm';
import { z } from 'zod';

import { desktopCommands, getMonitors, onProviderEmit } from '~/desktop';
import { getCoordinateDistance } from '~/utils';
import { createBaseProvider } from '../create-base-provider';
import type {
  NinjaProvider,
  NinjaProviderConfig,
} from './ninja-provider-types';

// `glazewm` is normalised to `ninja` rather than passed through, so the
// config hash and the type sent to the desktop process are the same
// whichever name the widget pack was written against.
const ninjaProviderConfigSchema = z.object({
  type: z.enum(['ninja', 'glazewm']).transform(() => 'ninja' as const),
});

/**
 * Raw state emitted by the desktop-side Ninja provider.
 *
 * The desktop process owns the IPC connection to the window manager and
 * forwards its state verbatim. Per-widget values (e.g. which monitor this
 * widget sits on) are derived here, since they differ per widget window.
 */
interface NinjaEmission {
  allMonitors: Monitor[];
  allWindows: Window[];
  focusedContainer: Container;
  tilingDirection: TilingDirection;
  bindingModes: BindingModeConfig[];
  isPaused: boolean;
}

export function createNinjaProvider(
  config: NinjaProviderConfig,
): NinjaProvider {
  const mergedConfig = ninjaProviderConfigSchema.parse(config);

  return createBaseProvider(mergedConfig, async queue => {
    const monitors = await getMonitors();

    return onProviderEmit<NinjaEmission>(
      mergedConfig,
      ({ configHash, result }) => {
        if ('error' in result) {
          queue.error(result.error);
          return;
        }

        const {
          allMonitors,
          allWindows,
          focusedContainer,
          tilingDirection,
          bindingModes,
          isPaused,
        } = result.output;

        // The WM reports no monitors while the displays are powering down
        // or coming back. Skipping the emission keeps the last good state
        // on screen; deriving from an empty list would throw in the
        // `reduce` below and leave this callback permanently broken.
        if (allMonitors.length === 0) {
          return;
        }

        // Tauri reports no current monitor when this widget's display has
        // gone away. Falling back to the origin just picks the nearest
        // monitor to 0,0 for one emission, until the widget is repositioned.
        const currentPosition = {
          x: monitors.currentMonitor?.x ?? 0,
          y: monitors.currentMonitor?.y ?? 0,
        };

        // Get the Ninja monitor that corresponds to this widget's
        // monitor.
        const currentNinjaMonitor = allMonitors.reduce((a, b) =>
          getCoordinateDistance(currentPosition, a) <
          getCoordinateDistance(currentPosition, b)
            ? a
            : b,
        );

        const focusedNinjaMonitor = allMonitors.find(
          monitor => monitor.hasFocus,
        );

        const allNinjaWorkspaces = allMonitors.flatMap(
          monitor => monitor.children,
        );

        const focusedNinjaWorkspace = focusedNinjaMonitor?.children.find(
          workspace => workspace.hasFocus,
        );

        const displayedNinjaWorkspace = currentNinjaMonitor.children.find(
          workspace => workspace.isDisplayed,
        );

        async function runCommand(
          command: string,
          subjectContainerId?: string,
        ): Promise<RunCommandResponse> {
          const response = await desktopCommands.callProviderFunction(
            configHash,
            {
              type: 'ninja',
              function: {
                name: 'run_command',
                args: { command, subjectContainerId },
              },
            },
          );

          // The Rust side returns the subject container ID as a bare
          // string.
          if (typeof response !== 'string') {
            throw new Error(
              `Unexpected response to command '${command}': ${response}`,
            );
          }

          return { subjectContainerId: response };
        }

        queue.output({
          displayedWorkspace: displayedNinjaWorkspace!,
          focusedWorkspace: focusedNinjaWorkspace!,
          currentWorkspaces: currentNinjaMonitor.children,
          allWorkspaces: allNinjaWorkspaces,
          focusedMonitor: focusedNinjaMonitor!,
          currentMonitor: currentNinjaMonitor,
          allMonitors,
          allWindows,
          focusedContainer,
          tilingDirection,
          bindingModes,
          isPaused,
          runCommand,
        });
      },
    );
  });
}
