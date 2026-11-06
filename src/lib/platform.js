// Dónde corre la interfaz. Es la misma en todos los casos; aquí se resuelve lo que cambia:
// - App de escritorio (Windows, Tauri): ventana propia, bandeja, diálogos del sistema.
// - App de Android (Tauri): diseño móvil, selector de carpetas propio, permisos, vibración.
// - Navegador contra nebula-server: sin ventana ni bandeja; las carpetas son las del servidor
//   (selector propio) y los .nzb se suben. En una pantalla estrecha usa el diseño móvil.
import { invoke, isTauri } from './transport.js';
import { open } from '@tauri-apps/plugin-dialog';
import { ui } from './state.svelte.js';

export { isTauri };
/** En el navegador, contra nebula-server. */
export const isServer = !isTauri;
/** La app de Android (no un navegador en un móvil). */
export const isAndroid = isTauri && /android/i.test(navigator.userAgent);
/** La app de escritorio (ventana propia, bandeja, diálogos del sistema). */
export const isDesktopApp = isTauri && !isAndroid;
/** La app de escritorio en Windows. */
export const isWindows = isDesktopApp && /windows/i.test(navigator.userAgent);
/** Diseño móvil: la app de Android o el navegador en una pantalla estrecha. */
export const isMobile = isAndroid || (isServer && window.matchMedia('(max-width: 760px)').matches);

/** Elige una carpeta. Fuera del escritorio abre el selector propio de Nebula (FolderPicker). */
export function pickFolder({ defaultPath, title } = {}) {
  if (isDesktopApp) return open({ directory: true, defaultPath: defaultPath || undefined, title });
  return new Promise((resolve) => {
    ui.folderPicker = { path: defaultPath || '', title: title ?? '', resolve };
  });
}

/** Elige uno o varios .nzb y devuelve sus rutas (en Android y el navegador se copian a la
 * carpeta de datos de Nebula). */
export async function pickNzbFiles({ title, filterName } = {}) {
  if (isDesktopApp) {
    const sel = await open({ multiple: true, title, filters: [{ name: filterName, extensions: ['nzb', 'gz'] }] });
    if (!sel) return [];
    return Array.isArray(sel) ? sel : [sel];
  }
  const files = await new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    if (isServer) input.accept = '.nzb,.gz';
    input.onchange = () => resolve([...(input.files ?? [])]);
    input.oncancel = () => resolve([]);
    input.click();
  });
  return importNzbFiles(files);
}

/** Copia .nzb del navegador o de otra app (objetos File) a Nebula y devuelve sus rutas. */
export async function importNzbFiles(files) {
  const paths = [];
  for (const file of files) {
    const data = new Uint8Array(await file.arrayBuffer());
    paths.push(await invoke('import_nzb', data, { headers: { 'x-name': encodeURIComponent(file.name) } }));
  }
  return paths;
}

/** Abre un enlace web: en la app, con el navegador del sistema; en el navegador, en otra pestaña. */
export function openUrl(url) {
  if (isTauri) return invoke('open_url', { url });
  window.open(url, '_blank', 'noopener');
  return Promise.resolve();
}
