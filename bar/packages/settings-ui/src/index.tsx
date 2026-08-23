/* @refresh reload */
import './index.css';
import { HashRouter, Route } from '@solidjs/router';
import { render } from 'solid-js/web';

import {
  AppLayout,
  syncThemeWithSystem,
  UserPacksProvider,
  WmSettingsProvider,
} from './common';
import { WidgetPage, WidgetPacksPage, WidgetPackPage } from './user-packs';
import { WmSettingsPage } from './wm';

// Match the OS light/dark setting before the first paint.
syncThemeWithSystem();

render(
  () => (
    <UserPacksProvider>
      <WmSettingsProvider>
        <HashRouter root={AppLayout}>
          <Route path="/" component={WidgetPacksPage} />
          <Route path="/packs/:packId" component={WidgetPackPage} />
          <Route
            path="/packs/:packId/:widgetName"
            component={WidgetPage}
          />
          <Route path="/wm" component={WmSettingsPage} />
        </HashRouter>
      </WmSettingsProvider>
    </UserPacksProvider>
  ),
  document.getElementById('root')!,
);
