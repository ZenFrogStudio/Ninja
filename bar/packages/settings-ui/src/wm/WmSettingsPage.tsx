import {
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  SwitchField,
  TextField,
} from '@glzr/components';
import { createSignal, For, Show } from 'solid-js';

import {
  AppBreadcrumbs,
  useWmSettings,
  type WmSettingChange,
} from '~/common';

/// Gap fields, mapped to their location in the config file.
const GAP_FIELDS = [
  { id: 'innerGap', path: ['gaps', 'inner_gap'], label: 'Inner gap' },
  { id: 'outerGapTop', path: ['gaps', 'outer_gap', 'top'], label: 'Top' },
  {
    id: 'outerGapRight',
    path: ['gaps', 'outer_gap', 'right'],
    label: 'Right',
  },
  {
    id: 'outerGapBottom',
    path: ['gaps', 'outer_gap', 'bottom'],
    label: 'Bottom',
  },
  {
    id: 'outerGapLeft',
    path: ['gaps', 'outer_gap', 'left'],
    label: 'Left',
  },
] as const;

const BORDER_GROUPS = [
  {
    group: 'focused_window',
    label: 'Focused window',
    enabledId: 'focusedEnabled',
    colorId: 'focusedColor',
  },
  {
    group: 'other_windows',
    label: 'Other windows',
    enabledId: 'otherEnabled',
    colorId: 'otherColor',
  },
] as const;

function borderPath(group: string, key: string) {
  return ['window_effects', group, 'border', key];
}

export function WmSettingsPage() {
  const { settings, save } = useWmSettings();

  const [edits, setEdits] = createSignal<Record<string, string>>({});
  const [isSaving, setIsSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  function valueOf(id: string, current: string | null | undefined) {
    return edits()[id] ?? current ?? '';
  }

  function setValue(id: string, value: string) {
    setEdits(prev => ({ ...prev, [id]: value }));
  }

  /// Length and colour values are quoted in the config (e.g. '20px').
  function quoted(value: string) {
    return `'${value.replace(/'/g, '')}'`;
  }

  async function onSave() {
    const pending = edits();
    const changes: WmSettingChange[] = [];

    if (pending.scale !== undefined) {
      changes.push({ path: ['gaps', 'scale'], value: pending.scale });
    }

    if (pending.scaleWithDpi !== undefined) {
      changes.push({
        path: ['gaps', 'scale_with_dpi'],
        value: pending.scaleWithDpi,
      });
    }

    for (const field of GAP_FIELDS) {
      const value = pending[field.id];

      if (value !== undefined) {
        changes.push({ path: [...field.path], value: quoted(value) });
      }
    }

    for (const border of BORDER_GROUPS) {
      const enabled = pending[border.enabledId];
      const color = pending[border.colorId];

      if (enabled !== undefined) {
        changes.push({
          path: borderPath(border.group, 'enabled'),
          value: enabled,
        });
      }

      if (color !== undefined) {
        changes.push({
          path: borderPath(border.group, 'color'),
          value: quoted(color),
        });
      }
    }

    if (changes.length === 0) {
      return;
    }

    setIsSaving(true);
    setError(null);

    try {
      await save(changes);
      setEdits({});
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <div class="container mx-auto px-6 py-6 space-y-6">
      <AppBreadcrumbs
        entries={[{ href: '/wm', content: 'Window manager' }]}
      />

      <Show
        when={settings()}
        fallback={<p class="text-muted-foreground">Loading settings…</p>}
      >
        {value => (
          <>
            <Card>
              <CardHeader>
                <CardTitle>Window borders</CardTitle>
                <CardDescription>
                  Highlights windows with a coloured outline. Windows 11
                  only. Thickness is set by the system and can't be changed
                  here.
                </CardDescription>
              </CardHeader>

              <CardContent class="space-y-6">
                <For each={BORDER_GROUPS}>
                  {border => (
                    <div class="space-y-3">
                      <SwitchField
                        id={border.enabledId}
                        label={`${border.label} border`}
                        value={
                          valueOf(
                            border.enabledId,
                            value().borders[border.enabledId],
                          ) === 'true'
                        }
                        onChange={next =>
                          setValue(border.enabledId, String(next))
                        }
                      />

                      <div class="flex items-center gap-3">
                        <input
                          type="color"
                          aria-label={`${border.label} border colour`}
                          class="h-9 w-14 rounded border border-input bg-background p-1 cursor-pointer"
                          value={
                            valueOf(
                              border.colorId,
                              value().borders[border.colorId],
                            ) || '#000000'
                          }
                          onInput={e =>
                            setValue(border.colorId, e.currentTarget.value)
                          }
                        />

                        <div class="flex-1">
                          <TextField
                            id={border.colorId}
                            label="Colour"
                            description="Hex value, e.g. #ff0000."
                            value={valueOf(
                              border.colorId,
                              value().borders[border.colorId],
                            )}
                            onChange={next =>
                              setValue(border.colorId, next)
                            }
                          />
                        </div>
                      </div>
                    </div>
                  )}
                </For>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle>Gaps</CardTitle>
                <CardDescription>
                  Spacing between windows and around the screen edge.
                </CardDescription>
              </CardHeader>

              <CardContent class="space-y-4">
                <TextField
                  id="gap-scale"
                  label="Gap scale"
                  description="Multiplies every gap below. 1 leaves them as written, 0.5 halves them, 0 removes them."
                  value={valueOf('scale', value().gaps.scale)}
                  onChange={next => setValue('scale', next)}
                />

                <SwitchField
                  id="scale-with-dpi"
                  label="Scale with display DPI"
                  description="Keeps gaps visually consistent across monitors with different scaling."
                  value={
                    valueOf('scaleWithDpi', value().gaps.scaleWithDpi) ===
                    'true'
                  }
                  onChange={next => setValue('scaleWithDpi', String(next))}
                />

                <div class="grid grid-cols-2 gap-4">
                  <For each={GAP_FIELDS}>
                    {field => (
                      <TextField
                        id={field.id}
                        label={field.label}
                        value={valueOf(field.id, value().gaps[field.id])}
                        onChange={next => setValue(field.id, next)}
                      />
                    )}
                  </For>
                </div>
              </CardContent>
            </Card>

            <Show when={error()}>
              {message => (
                <p class="text-sm text-destructive">{message()}</p>
              )}
            </Show>

            <div class="flex items-center gap-3">
              <Button
                disabled={isSaving() || Object.keys(edits()).length === 0}
                onClick={onSave}
              >
                {isSaving() ? 'Saving…' : 'Save changes'}
              </Button>

              <span class="text-xs text-muted-foreground truncate">
                {value().configPath}
              </span>
            </div>
          </>
        )}
      </Show>
    </div>
  );
}
