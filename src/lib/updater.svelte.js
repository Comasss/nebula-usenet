// Actualizaciones de Nebula: comprueba las Releases de GitHub al abrir la app (y cada pocas horas),
// avisa con una ventana que se puede cerrar u omitir y deja actualizar también desde Configuración.
// La descarga y la instalación las hace el motor (src-tauri/src/updater.rs, en la app).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { t } from './i18n.svelte.js';
import { isAndroid } from './platform.js';
import { ui, saveSettings, toast } from './state.svelte.js';
import { haptic } from './feedback.js';

const EVERY = 6 * 60 * 60 * 1000;

export const upd = $state({
  /** Resultado de la última comprobación (UpdateInfo del motor). */
  info: null,
  checking: false,
  checkedAt: 0,
  /** idle | downloading | ready | installing | permission | error */
  phase: 'idle',
  downloaded: 0,
  total: 0,
  /** Bytes por segundo de la descarga. */
  speed: 0,
  error: '',
  /** Ventana de aviso visible. */
  open: false,
});

/** Versión cuyo aviso se cerró en esta sesión: no se vuelve a abrir solo hasta reiniciar. */
let closedVersion = '';

export async function checkUpdates({ manual = false } = {}) {
  if (upd.checking) return;
  upd.checking = true;
  try {
    const info = await invoke('update_check');
    upd.info = info;
    upd.checkedAt = Date.now();
    if (info.available) {
      const muted = info.version === ui.settings?.updateSkipped || info.version === closedVersion;
      if (manual || !muted) upd.open = true;
    } else if (manual) {
      toast('success', t('update.upToDate', { version: info.current }));
    }
  } catch (e) {
    if (manual) toast('error', String(e));
    else console.warn('update_check', e);
  } finally {
    upd.checking = false;
  }
}

/** Descarga (si hace falta) e instala la versión encontrada. */
export async function startUpdate() {
  const info = upd.info;
  if (!info?.available) return;
  if (!info.kind) {
    // No hay archivo para este sistema: se abre la página de la versión.
    invoke('open_url', { url: info.url });
    return;
  }
  haptic('tap');
  upd.open = true;
  upd.error = '';
  if (upd.phase !== 'ready' && upd.phase !== 'permission') {
    upd.phase = 'downloading';
    upd.downloaded = 0;
    upd.total = info.assetSize;
    upd.speed = 0;
    try {
      await invoke('update_download');
    } catch (e) {
      if (upd.phase === 'downloading') {
        upd.phase = 'error';
        upd.error = String(e);
      }
      return;
    }
    if (upd.phase !== 'downloading') return; // cancelada
    upd.phase = 'ready';
  }
  await install();
}

/** Abre el instalador. En Windows Nebula se cierra y se vuelve a abrir actualizada. */
export async function install() {
  upd.phase = 'installing';
  upd.error = '';
  try {
    const r = await invoke('update_install');
    if (r.needsPermission) upd.phase = 'permission';
    // En Android el instalador del sistema queda encima; si se cancela, se puede volver a abrir.
    else if (isAndroid) upd.phase = 'ready';
  } catch (e) {
    upd.phase = 'error';
    upd.error = String(e);
  }
}

export function cancelDownload() {
  invoke('update_cancel');
  upd.phase = 'idle';
  upd.downloaded = 0;
}

/** Cierra el aviso. La descarga, si la hay, sigue y se ve en Configuración. */
export function closePrompt() {
  closedVersion = upd.info?.version ?? '';
  upd.open = false;
}

/** No volver a avisar de esta versión (sigue disponible en Configuración). */
export function skipVersion() {
  if (upd.info?.version) saveSettings({ updateSkipped: upd.info.version });
  upd.open = false;
}

export async function initUpdater() {
  let last = { at: 0, bytes: 0 };
  await listen('update-progress', (e) => {
    const { downloaded, total } = e.payload;
    const now = performance.now();
    if (last.at && now - last.at > 400) {
      const inst = ((downloaded - last.bytes) * 1000) / (now - last.at);
      upd.speed = upd.speed ? upd.speed * 0.6 + inst * 0.4 : inst;
      last = { at: now, bytes: downloaded };
    } else if (!last.at || downloaded < last.bytes) {
      last = { at: now, bytes: downloaded };
    }
    upd.downloaded = downloaded;
    upd.total = total;
  });

  // Android: al volver de la pantalla del permiso para instalar apps, queda listo para instalar.
  if (isAndroid) {
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && upd.phase === 'permission') upd.phase = 'ready';
    });
  }

  const auto = () => {
    if (ui.settings?.updateCheck !== false && upd.phase === 'idle') checkUpdates();
  };
  setTimeout(auto, 4000);
  setInterval(auto, EVERY);
}
