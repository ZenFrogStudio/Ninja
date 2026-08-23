import { invoke } from '@tauri-apps/api/core';
import {
  createContext,
  createResource,
  type JSX,
  useContext,
} from 'solid-js';

export interface WmGapsSettings {
  scaleWithDpi: string | null;
  scale: string | null;
  innerGap: string | null;
  outerGapTop: string | null;
  outerGapRight: string | null;
  outerGapBottom: string | null;
  outerGapLeft: string | null;
}

export interface WmBorderSettings {
  focusedEnabled: string | null;
  focusedColor: string | null;
  otherEnabled: string | null;
  otherColor: string | null;
}

export interface WmSettings {
  configPath: string;
  gaps: WmGapsSettings;
  borders: WmBorderSettings;
}

export interface WmSettingChange {
  /** Full path to the setting, e.g. `['gaps', 'scale']`. */
  path: string[];
  value: string;
}

type WmSettingsContextState = {
  settings: ReturnType<typeof createResource<WmSettings>>[0];
  save: (changes: WmSettingChange[]) => Promise<void>;
};

const WmSettingsContext = createContext<WmSettingsContextState>();

export function WmSettingsProvider(props: { children: JSX.Element }) {
  const [settings, { refetch }] = createResource<WmSettings>(() =>
    invoke<WmSettings>('read_wm_settings'),
  );

  async function save(changes: WmSettingChange[]) {
    // The desktop side edits the config in place and asks the window
    // manager to reload, so changes apply without a restart.
    await invoke<void>('write_wm_settings', { changes });
    await refetch();
  }

  return (
    <WmSettingsContext.Provider value={{ settings, save }}>
      {props.children}
    </WmSettingsContext.Provider>
  );
}

export function useWmSettings() {
  const context = useContext(WmSettingsContext);

  if (!context) {
    throw new Error(
      '`useWmSettings` must be used within a `WmSettingsProvider`.',
    );
  }

  return context;
}
