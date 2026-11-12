// A right-click menu in the app's own style. Pages describe their items; one menu is drawn for the whole window.

export interface MenuItem {
  label?: string;
  icon?: string;
  hint?: string;
  danger?: boolean;
  disabled?: boolean;
  sep?: boolean;
  run?: () => unknown;
}

export const menu = $state({ open: false, x: 0, y: 0, items: [] as MenuItem[] });

export function showMenu(e: MouseEvent, items: MenuItem[]) {
  const list = items.filter((i) => i.sep || i.label);
  if (!list.length) return;
  e.preventDefault();
  e.stopPropagation();
  menu.items = list;
  menu.x = e.clientX;
  menu.y = e.clientY;
  menu.open = true;
}
export const closeMenu = () => (menu.open = false);

/** `use:ctx={() => items}`: a right click on the element opens the menu with those items. */
export function ctx(node: HTMLElement, get: () => MenuItem[]) {
  const h = (e: MouseEvent) => showMenu(e, get());
  node.addEventListener('contextmenu', h);
  return { destroy: () => node.removeEventListener('contextmenu', h) };
}

/** Anywhere else: keep a small text menu for fields and selected text, and show nothing for the rest of the chrome. */
export function fallbackMenu(e: MouseEvent) {
  if (e.defaultPrevented) return;
  const t = e.target as HTMLElement | null;
  const field = t?.closest('input, textarea') as HTMLInputElement | null;
  const sel = window.getSelection()?.toString() ?? '';
  if (field) {
    const has = field.selectionStart !== field.selectionEnd;
    showMenu(e, [
      { label: 'Вырезать', hint: 'Ctrl+X', disabled: !has, run: () => document.execCommand('cut') },
      { label: 'Копировать', hint: 'Ctrl+C', disabled: !has, run: () => document.execCommand('copy') },
      {
        label: 'Вставить',
        hint: 'Ctrl+V',
        run: async () => {
          try {
            document.execCommand('insertText', false, await navigator.clipboard.readText());
          } catch {
            /* clipboard blocked */
          }
        },
      },
      { sep: true },
      { label: 'Выделить всё', hint: 'Ctrl+A', run: () => field.select() },
    ]);
  } else if (sel) {
    showMenu(e, [{ label: 'Копировать', hint: 'Ctrl+C', run: () => document.execCommand('copy') }]);
  } else {
    e.preventDefault();
  }
}
