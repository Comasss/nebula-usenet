// Respuesta a lo que hace el usuario: vibración breve (Android), ondas al tocar, pulsación larga
// y la animación de "enviado a Descargas".
import { invoke } from './transport.js';
import { isAndroid, isMobile } from './platform.js';
import { ui } from './state.svelte.js';

const animOn = () => ui.settings?.animations !== false;

/** Vibración breve: 'tap', 'tick', 'long', 'success' o 'error'. Solo en Android y si está activada. */
export function haptic(kind = 'tap') {
  if (!isAndroid || ui.settings?.haptics === false) return;
  invoke('haptic', { kind }).catch(() => {});
}

const RIPPLE = '.btn, .icon-btn, .tab, .tile, .s-row, .seg button, .chip-btn, [data-ripple]';

/** Onda que nace donde se toca (en los botones). Se instala una vez para toda la app. */
export function installRipples() {
  document.addEventListener(
    'pointerdown',
    (e) => {
      if (!animOn() || e.button > 0) return;
      const el = e.target.closest?.(RIPPLE);
      if (!el || el.disabled || el.closest('[data-no-ripple]')) return;
      const r = el.getBoundingClientRect();
      const size = Math.max(r.width, r.height) * 2.2;
      const dot = document.createElement('span');
      dot.className = 'ripple';
      dot.style.cssText = `width:${size}px;height:${size}px;left:${e.clientX - r.left - size / 2}px;top:${e.clientY - r.top - size / 2}px`;
      if (getComputedStyle(el).position === 'static') el.style.position = 'relative';
      el.classList.add('has-ripple');
      el.appendChild(dot);
      dot.addEventListener('animationend', () => dot.remove());
    },
    { passive: true, capture: true }
  );
}

/**
 * Acción `use:longpress`: mantener pulsado `ms` (o clic derecho) llama a `onlong`. Mientras se
 * mantiene, el elemento lleva la clase `holding` (para dibujar el progreso); el clic que sigue a
 * una pulsación larga se descarta.
 */
export function longpress(node, opts) {
  let o = opts;
  let timer = 0;
  let start = null;
  let fired = false;
  const cancel = () => {
    clearTimeout(timer);
    timer = 0;
    node.classList.remove('holding');
  };
  const down = (e) => {
    if (o?.enabled === false) return;
    if (e.button > 0) return;
    fired = false;
    start = { x: e.clientX, y: e.clientY };
    node.style.setProperty('--hold-ms', `${o?.ms ?? 480}ms`);
    node.classList.add('holding');
    timer = setTimeout(() => {
      fired = true;
      node.classList.remove('holding');
      haptic('long');
      o?.onlong?.(node);
    }, o?.ms ?? 480);
  };
  const move = (e) => {
    if (timer && start && Math.hypot(e.clientX - start.x, e.clientY - start.y) > 10) cancel();
  };
  const click = (e) => {
    if (fired) {
      fired = false;
      e.stopImmediatePropagation();
      e.preventDefault();
    }
  };
  const menu = (e) => {
    if (o?.enabled === false) return;
    e.preventDefault();
    cancel();
    // En Android el WebView cancela el toque (pointercancel) al detectar su propia pulsación larga,
    // a veces antes de que salte el temporizador: el menú contextual que llega después es la
    // pulsación larga. En escritorio es el clic derecho.
    if (!fired) {
      fired = isMobile; // tras la pulsación larga no debe colarse el clic
      if (isAndroid) haptic('long');
      o?.onlong?.(node);
    }
  };
  node.addEventListener('pointerdown', down);
  node.addEventListener('pointermove', move);
  node.addEventListener('pointerup', cancel);
  node.addEventListener('pointercancel', cancel);
  node.addEventListener('pointerleave', cancel);
  node.addEventListener('click', click, true);
  node.addEventListener('contextmenu', menu);
  return {
    update(next) {
      o = next;
    },
    destroy() {
      cancel();
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('pointermove', move);
      node.removeEventListener('pointerup', cancel);
      node.removeEventListener('pointercancel', cancel);
      node.removeEventListener('pointerleave', cancel);
      node.removeEventListener('click', click, true);
      node.removeEventListener('contextmenu', menu);
    },
  };
}

/**
 * Un icono vuela desde `from` hasta la pestaña (o el elemento del menú lateral) de `view`, que
 * rebota al recibirlo. Indica a dónde ha ido lo que se acaba de añadir.
 */
export function flyTo(from, view = 'downloads') {
  if (!animOn() || !from?.getBoundingClientRect) return;
  const target = document.querySelector(`[data-nav="${view}"]`);
  if (!target) return;
  const a = from.getBoundingClientRect();
  const b = target.getBoundingClientRect();
  if (!b.width) return;
  const dot = document.createElement('div');
  dot.className = 'fly-dot';
  dot.innerHTML =
    '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v12M6 11l6 6 6-6"/></svg>';
  document.body.appendChild(dot);
  const x0 = a.left + a.width / 2 - 17;
  const y0 = a.top + a.height / 2 - 17;
  const x1 = b.left + b.width / 2 - 17;
  const y1 = b.top + b.height / 2 - 17;
  const lift = Math.min(160, Math.abs(y1 - y0) * 0.35 + 60);
  const mid = { x: (x0 + x1) / 2, y: Math.min(y0, y1) - lift };
  const frames = [];
  for (let i = 0; i <= 12; i++) {
    const t = i / 12;
    const x = (1 - t) * (1 - t) * x0 + 2 * (1 - t) * t * mid.x + t * t * x1;
    const y = (1 - t) * (1 - t) * y0 + 2 * (1 - t) * t * mid.y + t * t * y1;
    frames.push({ transform: `translate(${x}px, ${y}px) scale(${1 - t * 0.45})`, opacity: t > 0.9 ? 0.4 : 1 });
  }
  const anim = dot.animate(frames, { duration: 650, easing: 'cubic-bezier(.45,.05,.55,.95)' });
  anim.onfinish = () => {
    dot.remove();
    target.classList.remove('catch');
    void target.offsetWidth;
    target.classList.add('catch');
    setTimeout(() => target.classList.remove('catch'), 600);
  };
}

/**
 * Abre el menú "Descargar con" junto a `anchor` y devuelve el método elegido (el `value` de
 * viaOptions) o null si se cierra sin elegir.
 */
export function chooseVia({ title = '', anchor = null, options, current = null } = {}) {
  ui.viaMenu?.resolve(null);
  return new Promise((resolve) => {
    const rect = anchor?.getBoundingClientRect?.() ?? null;
    ui.viaMenu = {
      title,
      options,
      current,
      rect: rect && { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, width: rect.width },
      resolve: (v) => {
        ui.viaMenu = null;
        resolve(v);
      },
    };
  });
}

/**
 * Hoja de acciones (pulsación larga en una descarga, botón ⋯). `actions`: [{ id, label, icon,
 * danger, disabled }]. Devuelve el id elegido o null.
 */
export function actionSheet({ title = '', subtitle = '', actions = [] } = {}) {
  ui.actionSheet?.resolve(null);
  return new Promise((resolve) => {
    ui.actionSheet = {
      title,
      subtitle,
      actions,
      resolve: (v) => {
        ui.actionSheet = null;
        resolve(v);
      },
    };
  });
}
