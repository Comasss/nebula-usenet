import { invoke, listen } from './transport.js';
import { t, setLanguage } from './i18n.svelte.js';
import { haptic } from './feedback.js';
import { remote, call, initRemote, routeShared } from './remote.svelte.js';

export const ui = $state({
  ready: false,
  view: 'downloads',
  settings: null,
  info: null,
  queue: {
    paused: false,
    speed: 0,
    remaining: 0,
    eta: 0,
    speedLimit: 0,
    activeConns: 0,
    jobs: [],
    servers: [],
    noServers: true,
  },
  history: [],
  logs: [],
  speedHistory: Array(120).fill(0),
  toasts: [],
  addItems: [],
  clipboard: [],
  setupOpen: false,
  dragging: false,
  dialog: null,
  // Menú "Descargar con" (pulsación larga en Descargar).
  viaMenu: null,
  // Hoja de acciones del móvil (pulsación larga en una descarga).
  actionSheet: null,
  // Solo en Android: selector de carpetas propio, página abierta dentro de Configuración, barra
  // inferior reducida al desplazarse y acceso a los archivos.
  folderPicker: null,
  settingsPage: null,
  navCompact: false,
  storage: true,
});

/** Duración de animaciones respetando la preferencia del usuario. */
export function d(ms) {
  return ui.settings && ui.settings.animations === false ? 0 : ms;
}

let toastSeq = 0;
export function toast(kind, message, timeout = 5500) {
  const id = ++toastSeq;
  if (kind === 'error') haptic('error');
  ui.toasts.push({ id, kind, message });
  if (ui.toasts.length > 5) ui.toasts.splice(0, ui.toasts.length - 5);
  if (timeout) setTimeout(() => dismissToast(id), timeout);
}

export function dismissToast(id) {
  const i = ui.toasts.findIndex((t) => t.id === id);
  if (i >= 0) ui.toasts.splice(i, 1);
}

export function queueAdd(items) {
  for (const it of items) {
    if (!ui.addItems.some((x) => x.value === it.value)) ui.addItems.push(it);
  }
}

/** Diálogo modal genérico. Devuelve {ok, checked, value}. */
export function ask(opts) {
  return new Promise((resolve) => {
    ui.dialog = {
      title: opts.title,
      message: opts.message ?? '',
      confirmLabel: opts.confirmLabel ?? t('common.ok'),
      cancelLabel: opts.cancelLabel ?? t('common.cancel'),
      danger: !!opts.danger,
      icon: opts.icon ?? (opts.danger ? 'alert-triangle' : 'info'),
      checkbox: opts.checkbox ?? null,
      checked: opts.checkedDefault ?? false,
      input: opts.input ?? null,
      value: opts.value ?? '',
      resolve,
    };
  });
}

export function closeDialog(result) {
  const dlg = ui.dialog;
  ui.dialog = null;
  dlg?.resolve(result);
}

export async function saveSettings(patch) {
  const next = { ...$state.snapshot(ui.settings), ...patch };
  const prev = ui.settings;
  ui.settings = next;
  try {
    await invoke('save_settings', { settings: next });
    return true;
  } catch (e) {
    ui.settings = prev;
    toast('error', String(e));
    return false;
  }
}

export async function refreshHistory() {
  ui.history = await call('get_history');
}

function handleOpenItems(items) {
  const adds = [];
  for (const it of items) {
    if (it.kind === 'clipboard') {
      if (!ui.clipboard.includes(it.value)) ui.clipboard.push(it.value);
    } else {
      adds.push({ kind: it.kind, value: it.value });
    }
  }
  queueAdd(adds);
}

/** Android: al volver a la app llegan los .nzb abiertos o compartidos y los enlaces del portapapeles. */
const seenClip = new Set();
async function resumed() {
  try {
    const r = await invoke('app_resumed');
    ui.storage = r.storage;
    // Si hay PCs emparejados, se pregunta si descargarlo aquí o en uno de ellos.
    routeShared([...r.files.map((value) => ({ kind: 'file', value })), ...r.urls.map((value) => ({ kind: 'url', value }))]);
    // Cada enlace se ofrece una sola vez, aunque siga en el portapapeles.
    for (const u of r.clipboard) {
      if (seenClip.has(u)) continue;
      seenClip.add(u);
      if (!ui.clipboard.includes(u)) ui.clipboard.push(u);
    }
  } catch (e) {
    console.error(e);
  }
}

export async function init() {
  const [settings, queue, history, logs, info] = await Promise.all([
    invoke('get_settings'),
    invoke('get_queue'),
    invoke('get_history'),
    invoke('get_logs'),
    invoke('app_info'),
  ]);
  ui.settings = settings;
  setLanguage(settings.language);
  ui.queue = queue;
  ui.history = history;
  ui.logs = logs;
  ui.info = info;

  // Mientras el móvil controla un PC, la cola y el historial que se ven son los del PC.
  await listen('queue', (e) => {
    if (!remote.active) ui.queue = e.payload;
  });
  await listen('history', () => {
    if (!remote.active) refreshHistory();
  });
  // Ajustes cambiados desde el motor (p. ej. el apagado "solo esta vez" al dispararse o desde la bandeja).
  await listen('settings', (e) => {
    ui.settings = e.payload;
  });
  await listen('log', (e) => {
    ui.logs.push(e.payload);
    if (ui.logs.length > 2000) ui.logs.splice(0, ui.logs.length - 2000);
  });
  await listen('toast', (e) => {
    toast(e.payload.kind, e.payload.message);
    if (e.payload.kind === 'success') haptic('success');
  });
  await listen('open-items', (e) => handleOpenItems(e.payload));
  await listen('clipboard-nzb', (e) => {
    for (const u of e.payload) if (!ui.clipboard.includes(u)) ui.clipboard.push(u);
  });

  setInterval(() => {
    ui.speedHistory.push(ui.queue.paused ? 0 : ui.queue.speed);
    if (ui.speedHistory.length > 120) ui.speedHistory.shift();
  }, 500);

  await initRemote().catch(console.error);

  if (/android/i.test(navigator.userAgent)) {
    await resumed();
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') resumed();
    });
  }

  // Se abre en la primera pestaña: Buscar si los indexadores están activos.
  ui.view = settings.featureIndexers ? 'search' : 'downloads';
  if (!settings.firstRunDone || !settings.featuresChosen) ui.setupOpen = true;
  ui.ready = true;
  await invoke('app_ready');
}
