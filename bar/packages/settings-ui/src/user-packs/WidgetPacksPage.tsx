import {
  Button,
  Card,
  CardContent,
  Dialog,
  DialogTrigger,
} from '@glzr/components';
import { useNavigate } from '@solidjs/router';
import { IconPlus } from '@tabler/icons-solidjs';
import { For, Show } from 'solid-js';

import {
  AppBreadcrumbs,
  type CreateWidgetArgs,
  useUserPacks,
} from '~/common';
import { CreateWidgetDialog } from './dialogs';

/// Name of the pack new widgets are created in.
///
/// Packs are still the on-disk storage unit, but they aren't a concept
/// the user deals with: everything they create goes into one, created on
/// demand and never named in the UI.
const DEFAULT_PACK_NAME = 'My widgets';

export function WidgetPacksPage() {
  const navigate = useNavigate();
  const userPacks = useUserPacks();

  /// Returns the pack that new widgets belong in, creating it if needed.
  async function defaultPackId() {
    const existing = userPacks.customPacks()?.[0];

    if (existing) {
      return existing.id;
    }

    const created = await userPacks.createPack({
      name: DEFAULT_PACK_NAME,
      version: '1.0.0',
      description: '',
      tags: [],
      repositoryUrl: '',
    });

    return created.id;
  }

  async function onCreateWidget(args: Omit<CreateWidgetArgs, 'packId'>) {
    const packId = await defaultPackId();
    await userPacks.createWidget({ ...args, packId });
    navigate(`/packs/${packId}/${args.name}`);
  }

  return (
    <div class="container mx-auto px-6 py-6">
      <AppBreadcrumbs entries={[]} />

      <div class="flex justify-between items-center mb-6">
        <h1 class="text-3xl font-bold">Widgets</h1>

        <Dialog>
          <DialogTrigger>
            <Button>
              <IconPlus class="mr-2 h-4 w-4" />
              Add widget
            </Button>
          </DialogTrigger>
          <CreateWidgetDialog onSubmit={onCreateWidget} />
        </Dialog>
      </div>

      <Show
        when={userPacks.allWidgets()?.length}
        fallback={
          <p class="text-muted-foreground">
            No widgets yet. Add one to get started.
          </p>
        }
      >
        <div class="space-y-2">
          <For each={userPacks.allWidgets()}>
            {entry => (
              <Card
                class="cursor-pointer transition-colors hover:bg-accent/50"
                onClick={() =>
                  navigate(`/packs/${entry.packId}/${entry.widget.name}`)
                }
              >
                <CardContent class="flex items-center justify-between py-4">
                  <span class="font-medium">{entry.widget.name}</span>

                  <span class="text-xs text-muted-foreground">
                    {entry.widget.presets.length}
                    {entry.widget.presets.length === 1
                      ? ' preset'
                      : ' presets'}
                  </span>
                </CardContent>
              </Card>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
