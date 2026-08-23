/**
 * Keeps the UI in step with the operating system's light/dark setting.
 *
 * The palette in `index.css` is scoped to `[data-kb-theme='dark']`, so
 * following the OS is a matter of setting that attribute.
 *
 * Uses the `prefers-color-scheme` media query rather than Tauri's window
 * theme API: it needs no IPC permission, and it fires on its own when the
 * user changes the system theme.
 *
 * Returns a function that stops listening.
 */
export function syncThemeWithSystem(): () => void {
  const query = window.matchMedia('(prefers-color-scheme: dark)');

  function apply(isDark: boolean) {
    document.documentElement.dataset.kbTheme = isDark ? 'dark' : 'light';
  }

  function onChange(event: MediaQueryListEvent) {
    apply(event.matches);
  }

  apply(query.matches);
  query.addEventListener('change', onChange);

  return () => query.removeEventListener('change', onChange);
}
