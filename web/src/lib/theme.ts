export type Theme = 'dark' | 'light' | 'system';

export function getStoredTheme(): Theme {
  if (typeof localStorage === 'undefined') return 'system';
  return (localStorage.getItem('dockup-theme') as Theme) || 'system';
}

export function applyTheme(theme: Theme): void {
  const isDark =
    theme === 'dark' ||
    (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.classList.toggle('dark', isDark);
}

export function setTheme(theme: Theme): void {
  localStorage.setItem('dockup-theme', theme);
  applyTheme(theme);
}

// Call this inline in <head> to prevent FOUC
export const themeInitScript = `
  (function() {
    const theme = localStorage.getItem('dockup-theme') || 'system';
    const isDark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
    if (isDark) document.documentElement.classList.add('dark');
  })();
`;
