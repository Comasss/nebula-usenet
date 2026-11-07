// Capa de transporte única entre la interfaz y el motor. Toda la interfaz llama al motor con
// `invoke(comando, args)` y escucha sus eventos con `listen(evento, handler)` desde aquí, nunca
// directamente desde @tauri-apps/api. El camino se elige al cargar:
// - Dentro de la app (Tauri): el IPC de Tauri.
// - En un navegador contra nebula-server: `POST /api/v1/invoke/<comando>` y un WebSocket con
//   los eventos (`/api/v1/events`), que se reconecta solo.
// En los dos casos el handler de `listen` recibe `{ event, payload }`, `listen` devuelve una
// promesa con la función para dejar de escuchar e `invoke` falla con el texto del error.
import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';

/** Si la interfaz corre dentro de la app (Tauri) y no en un navegador. */
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Estado de la sesión en el navegador: `needsAuth` se activa si el servidor pide entrar. */
export const session = { needsAuth: false, onAuthNeeded: null };

/** Llama a un comando del motor. Devuelve una promesa con su resultado (o su error, un texto). */
export function invoke(cmd, args, options) {
  return isTauri ? tauriInvoke(cmd, args, options) : httpInvoke(cmd, args, options);
}

/** Escucha un evento del motor ("queue", "history", "log"…). */
export function listen(event, handler) {
  return isTauri ? tauriListen(event, handler) : wsListen(event, handler);
}

// ---- Navegador (nebula-server) ----

async function httpInvoke(cmd, args, options) {
  const raw = args instanceof Uint8Array || args instanceof ArrayBuffer;
  const headers = { ...(options?.headers ?? {}) };
  if (!raw) headers['content-type'] = 'application/json';
  let res;
  try {
    res = await fetch(`api/v1/invoke/${encodeURIComponent(cmd)}`, {
      method: 'POST',
      headers,
      credentials: 'same-origin',
      body: raw ? args : JSON.stringify(args ?? {}),
    });
  } catch (e) {
    throw String(e?.message ?? e);
  }
  if (res.status === 401) authNeeded();
  const text = await res.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }
  if (!res.ok) throw data?.error ?? (typeof data === 'string' && data ? data : `HTTP ${res.status}`);
  return data;
}

function authNeeded() {
  if (session.needsAuth) return;
  session.needsAuth = true;
  session.onAuthNeeded?.();
}

const handlers = new Map();
let socket = null;
let retry = 0;
let retryTimer = null;

function connect() {
  if (socket || !handlers.size) return;
  const url = new URL('api/v1/events', location.href);
  url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(url);
  socket = ws;
  ws.onopen = () => {
    // Tras una reconexión, la interfaz vuelve a pedir el estado completo.
    if (retry > 0) dispatch('reconnected', null);
    retry = 0;
  };
  ws.onmessage = (m) => {
    try {
      const { event, payload } = JSON.parse(m.data);
      dispatch(event, payload ?? null);
    } catch {
      /* mensaje no válido: se ignora */
    }
  };
  ws.onclose = () => {
    if (socket === ws) socket = null;
    if (!handlers.size || session.needsAuth) return;
    // Reintenta cada vez más despacio, hasta 10 s.
    clearTimeout(retryTimer);
    retryTimer = setTimeout(connect, Math.min(10000, 500 * 2 ** retry++));
  };
}

function dispatch(event, payload) {
  for (const h of handlers.get(event) ?? []) {
    try {
      h({ event, payload });
    } catch (e) {
      console.error(e);
    }
  }
}

/** Vuelve a abrir los eventos (tras entrar). */
export function reconnectEvents() {
  socket?.close();
  socket = null;
  retry = 0;
  connect();
}

function wsListen(event, handler) {
  if (!handlers.has(event)) handlers.set(event, new Set());
  handlers.get(event).add(handler);
  connect();
  return Promise.resolve(() => handlers.get(event)?.delete(handler));
}
