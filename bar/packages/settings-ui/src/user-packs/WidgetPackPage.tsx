import {
  Button,
  Dialog,
  DialogTrigger,
  Card,
  CardContent,
  Table,
  TableHead,
  TableHeader,
  TableRow,
  TableBody,
  TableCell,
  AlertDialogTrigger,
  AlertDialog,
} from '@glzr/components';
import { useNavigate, useParams } from '@solidjs/router';
import { IconPlus, IconTrash } from '@tabler/icons-solidjs';
import { createMemo, Show } from 'solid-js';

import { AppBreadcrumbs, useUserPacks } from '~/common';
import { CreateWidgetDialog, DeleteWidgetDialog } from './dialogs';
import { WidgetPackForm } from './WidgetPackForm';

export function WidgetPackPage() {
  const params = useParams();
  const navigate = useNavigate();
  const userPacks = useUserPacks();

  const selectedPack = createMemo(() =>
    userPacks.allPacks()?.find(pack => pack.id === params.packId),
  );

  const isMarketplacePack = createMemo(
    () => selectedPack()?.type === 'marketplace',
  );

  return (
    <div class="container mx-auto pt-3.5 pb-32">
      <Show when={selectedPack()}>
        {pack => (
          <>
            <AppBreadcrumbs
              entries={[
                {
                  href: `/packs/${pack().id}`,
                  content: pack().id,
                },
              ]}
            />

            <h1 class="text-3xl font-bold mb-4">Widget Pack</h1>

            <WidgetPackForm
              pack={pack()}
              onChange={form => {
                if (form.isDirty() && !form.hasError()) {
                  userPacks.updatePack(pack().id, {
                    ...pack(),
                    ...form.value,
                  });
                }
              }}
              disabled={isMarketplacePack()}
            />

            <Card>
              <CardContent class="pt-6">
                <div class="flex justify-between items-center mb-4">
                  <h2 class="text-xl font-semibold">Widgets</h2>

                  <Dialog>
                    <DialogTrigger>
                      <Button
                        variant="outline"
                        disabled={isMarketplacePack()}
                      >
                        <IconPlus class="mr-2 h-4 w-4" />
                        Add widget
                      </Button>
                    </DialogTrigger>
                    <CreateWidgetDialog
                      onSubmit={args =>
                        userPacks.createWidget({
                          ...args,
                          packId: pack().id,
                        })
                      }
                    />
                  </Dialog>
                </div>

                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Name</TableHead>
                      <TableHead>HTML Path</TableHead>
                      <TableHead class="w-[100px]">Actions</TableHead>
                    </TableRow>
                  </TableHeader>

                  <TableBody>
                    {pack().widgets.map(config => (
                      <TableRow
                        class="cursor-pointer"
                        onClick={() =>
                          navigate(`/packs/${pack().id}/${config.name}`)
                        }
                      >
                        <TableCell>{config.name}</TableCell>
                        <TableCell>{config.htmlPath}</TableCell>
                        <TableCell onClick={e => e.stopPropagation()}>
                          <AlertDialog>
                            <AlertDialogTrigger
                              disabled={isMarketplacePack()}
                            >
                              <Button
                                variant="outline"
                                size="icon"
                                class="text-red-500 hover:text-red-600"
                                disabled={isMarketplacePack()}
                              >
                                <IconTrash class="h-4 w-4" />
                              </Button>
                            </AlertDialogTrigger>
                            <DeleteWidgetDialog
                              widget={config}
                              onDelete={() =>
                                userPacks.deleteWidget(
                                  pack().id,
                                  config.name,
                                )
                              }
                            />
                          </AlertDialog>
                        </TableCell>
                      </TableRow>
                    ))}

                    {pack().widgets.length === 0 && (
                      <TableRow>
                        <TableCell
                          colSpan={3}
                          class="text-center text-muted-foreground"
                        >
                          No widgets added yet
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          </>
        )}
      </Show>
    </div>
  );
}
