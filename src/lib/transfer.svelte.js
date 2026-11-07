// Pasar los ajustes a otro dispositivo con un QR que se escanea con el móvil:
// - Enviar: el QR lleva los ajustes y el otro dispositivo los importa.
// - Recibir: el QR lleva la dirección de este equipo en la Wi-Fi; el móvil lo escanea y envía los suyos.
// Si no se puede escanear, el mismo código se copia y se pega como texto.
import { invoke, listen } from './transport.js';
import { scan, cancel, checkPermissions, requestPermissions, Format } from '@tauri-apps/plugin-barcode-scanner';
import { t, setLanguage } from './i18n.svelte.js';
import { ui, toast } from './state.svelte.js';
import { haptic } from './feedback.js';

/** mode: 'send' | 'receive' | 'paste' | 'import' (confirmar) | 'push' (confirmar el envío por la Wi-Fi). */
export const tr = $state({ mode: null, qr: null, error: '', loading: false, busy: false, text: '', code: '', preview: null, scanning: false });

let offReceived = null;
let offClosed = null;

function reset(mode) {
  stopListening();
  Object.assign(tr, { mode, qr: null, error: '', loading: true, busy: false, text: '', code: '', preview: null });
}

function stopListening() {
  offReceived?.();
  offClosed?.();
  offReceived = offClosed = null;
}

export async function showSend() {
  reset('send');
  try {
    tr.qr = await invoke('transfer_export');
    if (!tr.qr.svg) tr.error = t('transfer.tooBig');
  } catch (e) {
    tr.error = String(e);
  }
  tr.loading = false;
}

export async function showReceive() {
  reset('receive');
  try {
    offReceived = await listen('transfer-received', (e) => {
      stopListening();
      haptic('success');
      openImport(e.payload.code, e.payload.preview);
    });
    offClosed = await listen('transfer-closed', () => {
      if (tr.mode === 'receive') tr.error = t('transfer.expired');
    });
    tr.qr = await invoke('transfer_receive_start');
  } catch (e) {
    stopListening();
    tr.error = String(e);
  }
  tr.loading = false;
}

export function showPaste() {
  reset('paste');
  tr.loading = false;
}

function openImport(code, preview) {
  Object.assign(tr, { mode: preview.kind === 'receiver' ? 'push' : 'import', code, preview, qr: null, error: '', loading: false, busy: false });
}

export function closeTransfer() {
  if (tr.busy) return;
  if (tr.mode === 'receive') invoke('transfer_receive_stop').catch(() => {});
  stopListening();
  tr.mode = null;
}

/** Código escaneado o pegado: ajustes para importar o un equipo que espera los nuestros. */
export async function handleCode(code) {
  try {
    openImport(code.trim(), await invoke('transfer_preview', { code: code.trim() }));
  } catch (e) {
    if (tr.mode === 'paste') tr.error = String(e);
    else toast('error', String(e));
  }
}

export async function confirmImport() {
  tr.busy = true;
  try {
    const s = await invoke('transfer_import', { code: tr.code });
    ui.settings = s;
    setLanguage(s.language);
    ui.setupOpen = false;
    tr.busy = false;
    tr.mode = null;
    haptic('success');
    toast('success', t('transfer.imported'));
  } catch (e) {
    tr.busy = false;
    tr.error = String(e);
  }
}

export async function confirmPush() {
  tr.busy = true;
  tr.error = '';
  try {
    await invoke('transfer_send', { target: tr.code });
    tr.busy = false;
    tr.mode = null;
    haptic('success');
    toast('success', t('transfer.sent'));
  } catch (e) {
    tr.busy = false;
    tr.error = String(e);
  }
}

export async function copyCode() {
  try {
    await navigator.clipboard.writeText(tr.qr.code);
    toast('success', t('transfer.copied'), 2500);
  } catch (e) {
    toast('error', String(e));
  }
}

/** Android: escanea con la cámara. La vista de la cámara queda detrás de la interfaz, que se vuelve transparente. */
export async function scanCode() {
  let perm = await checkPermissions().catch(() => 'denied');
  if (perm !== 'granted') perm = await requestPermissions().catch(() => 'denied');
  if (perm !== 'granted') {
    toast('warn', t('transfer.cameraDenied'));
    return;
  }
  tr.mode = null;
  tr.scanning = true;
  document.documentElement.classList.add('scanning');
  let content = null;
  try {
    content = (await scan({ windowed: true, formats: [Format.QRCode] }))?.content ?? null;
  } catch {
    // Cancelado.
  } finally {
    tr.scanning = false;
    document.documentElement.classList.remove('scanning');
  }
  if (content) {
    haptic('success');
    await handleCode(content);
  }
}

export function cancelScan() {
  cancel().catch(() => {});
}
