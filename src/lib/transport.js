// Capa de transporte única entre la interfaz y el motor. Toda la interfaz llama al motor con
// `invoke(comando, args)` y escucha sus eventos con `listen(evento, handler)` desde aquí, nunca
// directamente desde @tauri-apps/api, para que la misma interfaz pueda hablar con el motor por
// otro camino (el servidor web) sin tocar los componentes.
//
// En la app (Tauri) es el IPC de Tauri; el handler de `listen` recibe `{ event, payload }` y
// `listen` devuelve una promesa con la función para dejar de escuchar.
import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';

/** Si la interfaz corre dentro de la app (Tauri) y no en un navegador. */
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Llama a un comando del motor. Devuelve una promesa con su resultado (o su error, un texto). */
export function invoke(cmd, args, options) {
  return tauriInvoke(cmd, args, options);
}

/** Escucha un evento del motor ("queue", "history", "log"…). */
export function listen(event, handler) {
  return tauriListen(event, handler);
}
