import { z } from 'zod';

import { createBaseProvider } from '../create-base-provider';
import { desktopCommands, onProviderEmit } from '~/desktop';
import type {
  AudioOutput,
  AudioProvider,
  AudioProviderConfig,
  SetMuteOptions,
  SetVolumeOptions,
} from './audio-provider-types';

const audioProviderConfigSchema = z.object({
  type: z.literal('audio'),
});

export function createAudioProvider(
  config: AudioProviderConfig,
): AudioProvider {
  const mergedConfig = audioProviderConfigSchema.parse(config);

  return createBaseProvider(mergedConfig, async queue => {
    return onProviderEmit<AudioOutput>(
      mergedConfig,
      ({ configHash, result }) => {
        if ('error' in result) {
          queue.error(result.error);
        } else {
          queue.output({
            ...result.output,
            setVolume: async (
              volume: number,
              options?: SetVolumeOptions,
            ) => {
              await desktopCommands.callProviderFunction(configHash, {
                type: 'audio',
                function: {
                  name: 'set_volume',
                  args: { volume, deviceId: options?.deviceId },
                },
              });
            },
            setMute: async (mute: boolean, options?: SetMuteOptions) => {
              await desktopCommands.callProviderFunction(configHash, {
                type: 'audio',
                function: {
                  name: 'set_mute',
                  args: { mute, deviceId: options?.deviceId },
                },
              });
            },
          });
        }
      },
    );
  });
}
