// Control remoto.
// - En el PC: avisos de emparejamiento (código de 6 dígitos que hay que aprobar).
// - En Android: buscar PCs, emparejarlos y controlarlos. Mientras se controla un PC, Descargas,
//   Historial y Estadísticas enseñan las suyas y las acciones de esas pantallas van a él.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { t } from './i18n.svelte.js';
import { ui, ask, closeDialog, toast, queueAdd } from './state.svelte.js';
import { actionSheet, haptic } from './feedback.js';
import { viaOptions } from './debrid.js';

const isAndroid = /android/i.test(navigator.userAgent);

/** Lo que se puede pedir al PC (el resto de acciones de esas pantallas se ocultan). */
const OPS = new Set([
  'get_queue',
  'get_history',
  'get_stats',
  'get_indexer_stats',
  'get_job_files',
  'pause_job',
  'resume_job',
  'move_job',
  'set_job_priority',
  'set_job_category',
  'remove_job',
  'set_paused',
  'set_speed_limit',
  'remove_history',
  'clear_history',
]);

export const remote = $state({
  // PC que se está controlando: { id, name, status, error }. null = este móvil.
  active: null,
  pcs: [],
  found: [],
  searching: false,
  // Emparejamiento en curso en el móvil: { name, code } mientras espera al PC.
  pairing: null,
});

/** Igual que invoke, pero manda al PC lo que corresponde mientras se le controla. */
export function call(cmd, args = {}) {
  if (!remote.active || !OPS.has(cmd)) return invoke(cmd, args);
  const p = invoke('remote_call', { op: cmd, args });
  // Las acciones que fallan (PC apagado…) se avisan aquí; las lecturas las reintenta cada vista.
  if (!cmd.startsWith('get_')) p.catch((e) => toast('error', String(e), 6000));
  return p;
}

/** Nombre del móvil para el PC (Android pone el modelo en el user agent). */
function deviceName() {
  const m = navigator.userAgent.match(/Android [\d.]+; ([^;)]+?)(?: Build\/[^;)]*)?[;)]/);
  const model = m?.[1]?.trim();
  return model && model !== 'K' ? model : 'Android';
}

let localQueue = null;

async function backToLocal() {
  remote.active = null;
  ui.speedHistory = Array(120).fill(0);
  try {
    const [queue, history] = await Promise.all([invoke('get_queue'), invoke('get_history')]);
    ui.queue = queue;
    ui.history = history;
  } catch {
    if (localQueue) ui.queue = localQueue;
  }
}

// ---- Android ----

export async function refreshPcs() {
  remote.pcs = await invoke('remote_pcs').catch(() => []);
}

export async function searchPcs() {
  if (remote.searching) return;
  remote.searching = true;
  try {
    remote.found = await invoke('remote_discover');
  } catch (e) {
    toast('error', String(e));
  }
  remote.searching = false;
}

/** Empareja con el PC de `addr` (IP:puerto). Devuelve el PC o null. */
export async function pairPc(addr) {
  remote.pairing = { name: '', code: '', addr };
  try {
    const pc = await invoke('remote_pair', { addr, name: deviceName() });
    haptic('success');
    toast('success', t('remote.paired', { name: pc.name }));
    await refreshPcs();
    remote.found = remote.found.map((f) => (f.id === pc.id ? { ...f, paired: true } : f));
    return pc;
  } catch (e) {
    if (remote.pairing) toast('error', String(e), 8000);
    return null;
  } finally {
    remote.pairing = null;
  }
}

export function cancelPairing() {
  remote.pairing = null;
  invoke('remote_pair_cancel').catch(() => {});
}

export async function forgetPc(pc) {
  const r = await ask({ title: t('remote.forgetTitle', { name: pc.name }), message: t('remote.forgetText'), confirmLabel: t('remote.forget'), danger: true, icon: 'trash' });
  if (!r.ok) return;
  if (remote.active?.id === pc.id) await backToLocal();
  await invoke('remote_forget', { id: pc.id });
  await refreshPcs();
  remote.found = remote.found.map((f) => (f.id === pc.id ? { ...f, paired: false } : f));
}

/** Empieza a controlar un PC emparejado y abre sus descargas. */
export async function controlPc(pc) {
  try {
    if (!remote.active) localQueue = $state.snapshot(ui.queue);
    const st = await invoke('remote_connect', { id: pc.id });
    remote.active = { id: pc.id, name: st.name || pc.name, status: st.status, error: st.error ?? null };
    ui.speedHistory = Array(120).fill(0);
    ui.queue = { paused: false, speed: 0, remaining: 0, eta: 0, speedLimit: 0, activeConns: 0, jobs: [], servers: [], noServers: false };
    ui.history = [];
    ui.settingsPage = null;
    ui.view = 'downloads';
    haptic('success');
  } catch (e) {
    toast('error', String(e));
  }
}

export async function stopControl() {
  haptic('tap');
  await invoke('remote_disconnect').catch(() => {});
  await backToLocal();
}

/** Manda .nzb o enlaces al PC que se controla (o a `pcId`). */
export async function sendToPc(items, pcId = remote.active?.id) {
  const category = await chooseCategory(pcId, items.length > 1 ? t('remote.whereMany', { n: items.length }) : items[0]?.value.split(/[\\/]/).pop());
  if (category === null) return;
  for (const it of items) {
    try {
      const r = await invoke('remote_send', { id: pcId, kind: it.kind, value: it.value, category });
      const pc = remote.pcs.find((p) => p.id === pcId);
      haptic('success');
      toast('success', t('remote.sent', { name: r?.name ?? it.value, pc: pc?.name ?? remote.active?.name ?? '' }), 3500);
    } catch (e) {
      toast('error', String(e), 8000);
    }
  }
}

/**
 * Formas de descargar del PC controlado (sus servidores NNTP y servicios debrid), con el mismo
 * formato que viaOptions. El PC no manda contraseñas ni API keys, solo nombres e ids.
 */
export async function pcViaOptions() {
  if (!remote.active) return [];
  try {
    const v = await invoke('remote_call', { op: 'get_vias', args: {} });
    const settings = {
      servers: (v?.servers ?? []).map((s) => ({ ...s, enabled: true })),
      debrid: (v?.debrid ?? []).map((d) => ({ ...d, enabled: true, apiKey: '-' })),
    };
    return viaOptions(settings);
  } catch {
    // PC con una versión anterior: solo su método por defecto.
    return [];
  }
}

/**
 * Manda un resultado de búsqueda (su enlace) al PC controlado, con `via` o el método por defecto del PC.
 * El PC elige la categoría con sus propias reglas a partir de las categorías Newznab del resultado.
 */
export async function sendLinkToPc(url, name, via = null, newznab = []) {
  return invoke('remote_send', { id: remote.active?.id, kind: 'url', value: url, via, name, newznab });
}

/** Categorías del PC controlado (vacío si no tiene o si es de una versión anterior). */
export async function pcCategories(pcId = remote.active?.id) {
  try {
    const v = remote.active?.id === pcId ? await invoke('remote_call', { op: 'get_vias', args: {} }) : null;
    return v?.categories ?? [];
  } catch {
    return [];
  }
}

/** Si el PC tiene categorías, pregunta en cuál meter lo que se le manda. null = cancelado. */
async function chooseCategory(pcId, title) {
  const cats = await pcCategories(pcId);
  if (!cats.length) return '*';
  const r = await actionSheet({
    title: t('cats.chooseTitle'),
    subtitle: title,
    actions: [{ id: '*', label: t('cats.default'), icon: 'inbox' }, ...cats.map((c) => ({ id: c, label: c, icon: 'tag' }))],
  });
  return r ?? null;
}

/** Añadir desde Descargas: al PC si se le está controlando, si no a este móvil. */
export function addItems(items) {
  if (!items.length) return;
  if (remote.active) sendToPc(items);
  else queueAdd(items);
}

/** NZB compartido con Nebula: si hay PCs emparejados, se pregunta dónde descargarlo. */
export async function routeShared(items) {
  if (!items.length) return;
  if (!remote.pcs.length) await refreshPcs();
  if (!remote.pcs.length) {
    queueAdd(items);
    return;
  }
  const first = items[0].value.split(/[\\/]/).pop();
  const target = await actionSheet({
    title: t('remote.whereTitle'),
    subtitle: items.length > 1 ? t('remote.whereMany', { n: items.length }) : first,
    actions: [
      { id: '', label: t('remote.thisPhone'), icon: 'smartphone' },
      ...remote.pcs.map((p) => ({ id: p.id, label: t('remote.onPc', { name: p.name }), icon: 'monitor' })),
    ],
  });
  if (target === null) return;
  if (target === '') queueAdd(items);
  else sendToPc(items, target);
}

// ---- PC: peticiones de emparejamiento ----

let pairDialog = null;

async function onPairRequest(p) {
  pairDialog = p.req;
  const r = await ask({
    title: t('remote.pairTitle', { name: p.name }),
    message: t('remote.pairText', { name: p.name, code: `${p.code.slice(0, 3)} ${p.code.slice(3)}`, from: p.from }),
    confirmLabel: t('remote.approve'),
    cancelLabel: t('remote.reject'),
    icon: 'smartphone',
  });
  if (pairDialog === p.req) pairDialog = null;
  invoke('remote_pair_answer', { req: p.req, ok: !!r.ok }).catch(() => {});
}

export async function initRemote() {
  if (isAndroid) {
    await refreshPcs();
    await listen('remote-pair-code', (e) => {
      if (remote.pairing) remote.pairing = { ...remote.pairing, ...e.payload };
    });
    await listen('remote-state', (e) => {
      const st = e.payload;
      if (!remote.active || remote.active.id !== st.id) return;
      if (st.status === 'stopped') return;
      const was = remote.active.status;
      remote.active = { ...remote.active, name: st.name || remote.active.name, status: st.status, error: st.error ?? null };
      if (st.status === 'online' && was === 'offline') toast('success', t('remote.reconnected', { name: remote.active.name }), 2500);
      if (st.status === 'unauthorized') {
        toast('error', st.error, 9000);
        refreshPcs();
        backToLocal();
      }
    });
    await listen('remote-queue', (e) => {
      if (remote.active) ui.queue = e.payload;
    });
    await listen('remote-history', (e) => {
      if (remote.active) ui.history = e.payload;
    });
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && remote.active) invoke('remote_wake').catch(() => {});
    });
  } else {
    await listen('remote-pair-request', (e) => onPairRequest(e.payload));
    await listen('remote-pair-closed', (e) => {
      // El móvil se ha ido o ha pasado el tiempo: se cierra el aviso si sigue abierto.
      if (pairDialog === e.payload.req && ui.dialog) {
        pairDialog = null;
        closeDialog({ ok: false });
      }
    });
  }
}
