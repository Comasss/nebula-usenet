// Diferencias entre el escritorio (Windows) y el móvil (Android). La interfaz es la misma; aquí
// se resuelve lo que en Android funciona de otra forma: elegir carpetas y archivos.
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { ui } from './state.svelte.js';

export const isAndroid = /android/i.test(navigator.userAgent);
export const isWindows = !isAndroid && /windows/i.test(navigator.userAgent);

/** Elige una carpeta. En Android abre el selector propio de Nebula (FolderPicker). */
export function pickFolder({ defaultPath, title } = {}) {
  if (!isAndroid) return open({ directory: true, defaultPath: defaultPath || undefined, title });
  return new Promise((resolve) => {
    ui.folderPicker = { path: defaultPath || '', title: title ?? '', resolve };
  });
}

/** Elige uno o varios .nzb y devuelve sus rutas (en Android se copian a la carpeta de la app). */
export async function pickNzbFiles({ title, filterName } = {}) {
  if (!isAndroid) {
    const sel = await open({ multiple: true, title, filters: [{ name: filterName, extensions: ['nzb', 'gz'] }] });
    if (!sel) return [];
    return Array.isArray(sel) ? sel : [sel];
  }
  const files = await new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    input.onchange = () => resolve([...(input.files ?? [])]);
    input.oncancel = () => resolve([]);
    input.click();
  });
  const paths = [];
  for (const file of files) {
    const data = new Uint8Array(await file.arrayBuffer());
    paths.push(await invoke('import_nzb', data, { headers: { 'x-name': encodeURIComponent(file.name) } }));
  }
  return paths;
}
