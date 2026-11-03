<script>
  // Navegación de la versión móvil: barra flotante de cristal con las vistas principales. Una
  // "lente" marca la pestaña activa; se puede arrastrar el dedo por la barra para cambiar de
  // pestaña. Indexadores, servidores, debrid y el registro están dentro de Configuración.
  import { t } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';
  import { ui } from '../lib/state.svelte.js';
  import { remote } from '../lib/remote.svelte.js';
  import { upd } from '../lib/updater.svelte.js';
  import { haptic } from '../lib/feedback.js';
  import { onMount } from 'svelte';

  // La barra entra cuando la primera vista ya está pintada, así el cristal no compite con el
  // arranque de la app.
  let shown = $state(false);
  onMount(() => {
    const id = requestAnimationFrame(() => requestAnimationFrame(() => (shown = true)));
    return () => cancelAnimationFrame(id);
  });

  const ALL = [
    { id: 'search', icon: 'search', feature: 'indexers' },
    { id: 'downloads', icon: 'download', feature: 'nntp' },
    { id: 'history', icon: 'history', feature: 'nntp' },
    { id: 'stats', icon: 'chart' },
    { id: 'settings', icon: 'settings' },
  ];
  // Vistas que viven dentro de Configuración.
  const IN_SETTINGS = ['settings', 'indexers', 'servers', 'debrid', 'logs'];

  const items = $derived(
    ALL.filter((i) => !i.feature || (i.feature === 'nntp' ? !!remote.active || ui.settings?.featureNntp !== false : ui.settings?.featureIndexers))
  );
  const activeId = $derived(IN_SETTINGS.includes(ui.view) ? 'settings' : ui.view);
  const active = $derived(Math.max(0, items.findIndex((i) => i.id === activeId)));

  const queueCount = $derived(ui.queue.jobs.length);
  const failedCount = $derived(ui.history.filter((h) => h.status === 'failed').length);
  const errorLogs = $derived(ui.logs.filter((l) => l.level === 'error').length);

  // La pestaña activa es más ancha y es la única que muestra su nombre.
  const PAD = 6;
  const GROW = 2;
  let width = $state(0);
  let bar;
  const unit = $derived(items.length ? (width - PAD * 2) / (items.length - 1 + GROW) : 0);
  const tabLeft = (i, act) => PAD + i * unit + (i > act ? (GROW - 1) * unit : 0);
  const tabWidth = (i, act) => (i === act ? GROW * unit : unit);

  // Arrastre de la lente.
  let drag = $state(null); // { x, index } mientras se arrastra
  let press = null;
  let squish = $state(false);
  let lastActive = -1;

  $effect(() => {
    const a = active;
    if (lastActive >= 0 && a !== lastActive) {
      squish = true;
      setTimeout(() => (squish = false), 260);
    }
    lastActive = a;
  });

  const lens = $derived(drag ? { x: drag.x, w: unit * 1.35 } : { x: tabLeft(active, active), w: tabWidth(active, active) });

  function indexAt(clientX) {
    const x = clientX - bar.getBoundingClientRect().left;
    for (let i = items.length - 1; i > 0; i--) if (x >= tabLeft(i, active)) return i;
    return 0;
  }

  function down(e) {
    press = { x: e.clientX, id: e.pointerId, moved: false, index: indexAt(e.clientX) };
  }
  function move(e) {
    if (!press || e.pointerId !== press.id) return;
    if (!press.moved && Math.abs(e.clientX - press.x) < 8) return;
    if (!press.moved) {
      press.moved = true;
      bar.setPointerCapture?.(e.pointerId);
    }
    const r = bar.getBoundingClientRect();
    const w = unit * 1.35;
    const x = Math.max(PAD, Math.min(width - PAD - w, e.clientX - r.left - w / 2));
    const index = indexAt(e.clientX);
    if (index !== press.index) {
      press.index = index;
      haptic('tick');
    }
    drag = { x, index };
  }
  function up(e) {
    if (!press || e.pointerId !== press.id) return;
    const p = press;
    press = null;
    if (p.moved) {
      drag = null;
      go(items[p.index].id, true);
    }
  }

  function go(id, fromDrag = false) {
    if (!fromDrag && press?.moved) return;
    if (id !== activeId || ui.settingsPage) haptic('tap');
    if (id === 'settings' && activeId === 'settings') ui.settingsPage = null;
    ui.view = id;
  }
</script>

{#snippet badge(id)}
  {#if id === 'downloads' && queueCount}
    {#key queueCount}<span class="badge">{queueCount}</span>{/key}
  {:else if id === 'history' && failedCount}
    <span class="badge err">{failedCount}</span>
  {:else if id === 'settings' && errorLogs}
    <span class="badge err small"></span>
  {:else if id === 'settings' && upd.info?.available}
    <span class="badge small upd"></span>
  {/if}
{/snippet}

<div class="fade" aria-hidden="true"></div>
<nav
  class="mnav glass"
  class:shown
  class:compact={ui.navCompact && !drag}
  bind:this={bar}
  bind:clientWidth={width}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
>
  {#if unit}
    <div class="lens" class:dragging={!!drag} class:squish style="transform: translateX({lens.x}px); width: {lens.w}px"></div>
  {/if}
  {#each items as it, i (it.id)}
    <button
      class="tab"
      class:active={drag ? drag.index === i : i === active}
      class:wide={i === active}
      data-nav={it.id}
      data-no-ripple
      onclick={() => go(it.id)}
      aria-label={t(`nav.${it.id}`)}>
      <span class="ic"><Icon name={it.icon} size={21} stroke={i === active ? 2.3 : 2} />{@render badge(it.id)}</span>
      <span class="label">{t(`nav.${it.id}`)}</span>
    </button>
  {/each}
</nav>

<style>
  .fade {
    position: fixed;
    left: 0; right: 0; bottom: 0;
    height: calc(110px + env(safe-area-inset-bottom, 0px));
    background: linear-gradient(180deg, transparent, rgba(9, 10, 15, 0.7) 55%, rgba(9, 10, 15, 0.92));
    pointer-events: none;
    z-index: 58;
  }
  .mnav {
    position: fixed;
    left: 14px; right: 14px;
    bottom: calc(12px + env(safe-area-inset-bottom, 0px));
    z-index: 60;
    display: flex;
    height: 66px;
    padding: 0 6px;
    border-radius: 999px;
    touch-action: pan-y;
    /* Capa propia y solo transform/opacity: el desenfoque no se recalcula con cada cambio. */
    contain: layout style;
    will-change: transform;
    opacity: 0;
    transform: translateY(calc(100% + 24px));
    transition: transform 0.45s var(--ease), opacity 0.3s;
  }
  .mnav.shown { opacity: 1; transform: none; }
  .mnav.shown.compact { transform: translateY(6px) scale(0.88); }
  /* Brillo del borde superior, como el canto de un cristal. */
  .mnav::before {
    content: '';
    position: absolute; inset: 0;
    border-radius: inherit;
    pointer-events: none;
    background: linear-gradient(115deg, rgba(255, 255, 255, 0.14), transparent 28%, transparent 70%, rgba(255, 255, 255, 0.06));
  }
  .lens {
    position: absolute;
    top: 5px; bottom: 5px; left: 0;
    border-radius: 999px;
    background:
      radial-gradient(120% 90% at 50% 0%, rgba(255, 255, 255, 0.16), transparent 60%),
      linear-gradient(135deg, rgba(var(--accent-rgb), 0.38), rgba(var(--accent-rgb), 0.16));
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.28), inset 0 0 0 1px rgba(var(--accent-rgb), 0.35), 0 6px 18px -6px rgba(var(--accent-rgb), 0.8);
    transition: transform 0.45s cubic-bezier(0.34, 1.36, 0.64, 1), width 0.35s var(--ease), scale 0.25s var(--ease);
    will-change: transform;
    pointer-events: none;
  }
  .lens.squish { animation: squish 0.42s var(--ease); }
  .lens.dragging {
    transition: transform 0.08s linear, width 0.2s var(--ease), scale 0.25s var(--ease);
    scale: 1.14 1.1;
    background:
      radial-gradient(120% 90% at 50% 0%, rgba(255, 255, 255, 0.24), transparent 60%),
      linear-gradient(135deg, rgba(var(--accent-rgb), 0.3), rgba(255, 255, 255, 0.06));
  }
  @keyframes squish {
    0% { scale: 1 1; }
    40% { scale: 1.22 0.86; }
    100% { scale: 1 1; }
  }
  .tab {
    position: relative;
    z-index: 1;
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 11px;
    font-weight: 650;
    letter-spacing: -0.01em;
    cursor: pointer;
    transition: color 0.25s;
    -webkit-tap-highlight-color: transparent;
  }
  .tab.wide { flex-grow: 2; }
  .tab.active { color: var(--text); }
  .tab.active .ic { color: var(--accent-2); transform: translateY(-1px) scale(1.06); }
  .tab:active .ic { transform: scale(0.86); }
  .ic {
    position: relative;
    display: grid;
    place-items: center;
    height: 26px;
    transition: transform 0.35s cubic-bezier(0.34, 1.56, 0.64, 1), color 0.25s;
  }
  .label {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-height: 0;
    opacity: 0;
    transform: translateY(4px);
    transition: opacity 0.25s, max-height 0.3s var(--ease), transform 0.3s var(--ease);
  }
  .tab.wide .label { max-height: 16px; opacity: 1; transform: none; }
  .badge {
    position: absolute; top: -4px; left: 13px;
    min-width: 17px; height: 17px; padding: 0 5px;
    border-radius: 999px;
    display: grid; place-items: center;
    font-size: 10px; font-weight: 750; color: #fff;
    background: var(--grad-diag);
    box-shadow: 0 0 0 2px rgba(18, 20, 30, 0.9);
    animation: pop 0.45s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .badge.err { background: #dc5050; }
  .badge.small.upd { background: var(--accent-2); }
  .badge.small { min-width: 9px; width: 9px; height: 9px; padding: 0; top: 0; left: 17px; background: var(--err); }
  @keyframes pop { from { transform: scale(0.3); } }
</style>
