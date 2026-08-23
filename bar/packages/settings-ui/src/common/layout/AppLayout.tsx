import {
  Resizable,
  ResizableHandle,
  ResizablePanel,
  ToastList,
  ToastRegion,
} from '@glzr/components';
import { createSignal, onMount, type JSX } from 'solid-js';
import { RouteSectionProps } from '@solidjs/router';

import { Sidebar } from './Sidebar';

export interface AppLayoutProps {
  children: JSX.Element;
}

export function AppLayout(props: AppLayoutProps & RouteSectionProps) {
  const [sizes, setSizes] = createSignal<number[]>([0.2, 0.8]);

  // Disable the right-click context menu.
  onMount(() => {
    document.addEventListener('contextmenu', (e: MouseEvent) => {
      e.preventDefault();
    });
  });

  return (
    <>
      <Resizable sizes={sizes()} onSizesChange={setSizes}>
        <Sidebar
          initialSize={sizes()[0]}
          onCollapseClick={() => setSizes([0, 1])}
        />

        <ResizableHandle withHandle />

        <ResizablePanel
          initialSize={sizes()[1]}
          class="overflow-y-auto bg-background"
        >
          {props.children}
        </ResizablePanel>
      </Resizable>

      <ToastRegion>
        <ToastList />
      </ToastRegion>
    </>
  );
}
