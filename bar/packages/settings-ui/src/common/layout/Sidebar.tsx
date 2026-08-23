import { Button, cn, ResizablePanel, Separator } from '@glzr/components';
import {
  IconChevronsLeft,
  IconHome,
  IconLayoutBoard,
  IconLayoutGrid,
} from '@tabler/icons-solidjs';
import { createSignal, For } from 'solid-js';

import { useUserPacks } from '~/common';
import { SidebarItem } from './SidebarItem';

export interface SidebarProps {
  initialSize: number;
  onCollapseClick: () => void;
}

export function Sidebar(props: SidebarProps) {
  const userPacks = useUserPacks();
  const [isCollapsed, setIsCollapsed] = createSignal(false);

  return (
    <ResizablePanel
      minSize={0.1}
      maxSize={0.2}
      initialSize={props.initialSize}
      collapsible
      onCollapse={size => setIsCollapsed(size === 0)}
      onExpand={() => setIsCollapsed(false)}
      class={cn(
        'overflow-y-auto overflow-x-hidden bg-card/40 flex flex-col',
        isCollapsed() &&
          'min-w-[50px] transition-all duration-300 ease-in-out',
      )}
    >
      <div class="flex justify-between items-center">
        <img
          src="/logo-128x128.png"
          alt="Ninja logo"
          class={cn(
            'app-logo w-8 h-8 m-2 ml-4 transition-all',
            isCollapsed() && 'w-5 h-5',
          )}
        />

        <Button
          onClick={props.onCollapseClick}
          class={cn(
            'mr-2 p-2 text-muted-foreground',
            isCollapsed() && 'hidden',
          )}
          variant="ghost"
        >
          <IconChevronsLeft class="size-4" />
        </Button>
      </div>

      <Separator />

      <SidebarItem
        isCollapsed={isCollapsed()}
        icon={<IconHome class="size-6" />}
        tooltip="Home"
        href="/"
      >
        <div class="truncate">My widgets</div>
      </SidebarItem>

      <SidebarItem
        isCollapsed={isCollapsed()}
        icon={<IconLayoutGrid class="size-6" />}
        tooltip="Window manager"
        href="/wm"
      >
        <div class="truncate">Window manager</div>
      </SidebarItem>

      {!isCollapsed() && (
        <h3 class="px-4 text-xs font-medium text-muted-foreground truncate mt-3">
          Widgets
        </h3>
      )}

      <For each={userPacks.allWidgets()}>
        {entry => (
          <SidebarItem
            isCollapsed={isCollapsed()}
            icon={<IconLayoutBoard class="size-6" />}
            tooltip={entry.widget.name}
            href={`/packs/${entry.packId}/${entry.widget.name}`}
          >
            <div class="truncate">{entry.widget.name}</div>
          </SidebarItem>
        )}
      </For>
    </ResizablePanel>
  );
}
