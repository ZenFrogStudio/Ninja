import {
  Button,
  DialogFooter,
  DialogHeader,
  DialogContent,
  Dialog,
  DialogTitle,
  DialogDescription,
} from '@glzr/components';
import { FormState } from 'smorf';
import { createSignal } from 'solid-js';

import { CreateWidgetArgs } from '~/common';
import { CreateWidgetForm } from '../CreateWidgetForm';

export type CreateWidgetDialogProps = {
  /// Pack the widget is created in. Optional because the caller may
  /// resolve it lazily, creating a default pack on demand.
  packId?: string;
  onSubmit: (widget: Omit<CreateWidgetArgs, 'packId'>) => void;
};

export function CreateWidgetDialog(props: CreateWidgetDialogProps) {
  const [form, setForm] = createSignal<FormState<CreateWidgetArgs> | null>(
    null,
  );

  return (
    <DialogContent>
      <DialogHeader>
        <DialogTitle>Add new widget</DialogTitle>
        <DialogDescription>
          Widgets are HTML, CSS and JavaScript. Pick a starting template
          below.
        </DialogDescription>
      </DialogHeader>

      <div class="py-4">
        <CreateWidgetForm onChange={setForm} />
      </div>

      <DialogFooter>
        <Dialog.CloseButton>
          <Button variant="outline">Cancel</Button>
        </Dialog.CloseButton>

        <Dialog.CloseButton
          onClick={() =>
            props.onSubmit(
              form()!.value as Omit<CreateWidgetArgs, 'packId'>,
            )
          }
        >
          <Button disabled={!form()?.value.name.trim()}>
            Create widget
          </Button>
        </Dialog.CloseButton>
      </DialogFooter>
    </DialogContent>
  );
}
