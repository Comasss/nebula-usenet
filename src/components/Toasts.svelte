<script>
  import { t as tr } from '../lib/i18n.svelte.js';
  import { fly, fade } from 'svelte/transition';
  import { flip } from 'svelte/animate';
  import { cubicOut } from 'svelte/easing';
  import Icon from './Icon.svelte';
  import { ui, dismissToast, d } from '../lib/state.svelte.js';

  import { isMobile } from '../lib/platform.js';

  const ICON = { success: 'check-circle', error: 'x-circle', warn: 'alert-triangle', info: 'info' };

  // Móvil: los avisos bajan desde arriba y se descartan deslizándolos hacia un lado.
  let swipe = $state({ id: null, dx: 0 });
  let start = null;
  function down(e, id) {
    start = { x: e.clientX, id };
  }
  function move(e) {
    if (!start) return;
    const dx = e.clientX - start.x;
    if (Math.abs(dx) > 4) swipe = { id: start.id, dx };
  }
  function up() {
    if (!start) return;
    if (Math.abs(swipe.dx) > 90 && swipe.id === start.id) dismissToast(start.id);
    start = null;
    swipe = { id: null, dx: 0 };
  }
</script>

<svelte:window onpointermove={move} onpointerup={up} onpointercancel={up} />

<div class="toasts" class:top={isMobile}>
  {#each ui.toasts as t (t.id)}
    <div
      class="toast {t.kind}"
      class:glass={isMobile}
      class:dragging={swipe.id === t.id}
      style={swipe.id === t.id ? `transform: translateX(${swipe.dx}px); opacity: ${1 - Math.min(0.8, Math.abs(swipe.dx) / 220)}` : ''}
      onpointerdown={(e) => down(e, t.id)}
      animate:flip={{ duration: d(300) }}
      in:fly={isMobile ? { y: -36, duration: d(380), easing: cubicOut } : { x: 40, duration: d(320), easing: cubicOut }}
      out:fade={{ duration: d(180) }}>
      <Icon name={ICON[t.kind] ?? 'info'} size={18} />
      <div class="msg selectable">{t.message}</div>
      <button class="icon-btn close" onclick={() => dismissToast(t.id)} aria-label={tr('common.close')}><Icon name="x" size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 20px;
    bottom: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    z-index: 300;
    width: 380px;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 10px 12px 14px;
    border-radius: 12px;
    background: rgba(24, 27, 37, 0.96);
    border: 1px solid var(--border-2);
    box-shadow: var(--shadow);
    backdrop-filter: blur(10px);
    font-size: 13px;
    line-height: 1.4;
  }
  .toast :global(svg) { margin-top: 1px; }
  .toast.success :global(svg:first-child) { color: var(--ok); }
  .toast.error { border-color: rgba(248, 113, 113, 0.35); }
  .toast.error :global(svg:first-child) { color: var(--err); }
  .toast.warn :global(svg:first-child) { color: var(--warn); }
  .toast.info :global(svg:first-child) { color: var(--info); }
  .msg { flex: 1; min-width: 0; word-break: break-word; }
  .close { width: 24px; height: 24px; margin: -3px 0; }

  .toast { transition: transform 0.3s var(--ease), opacity 0.3s; touch-action: pan-y; }
  .toast.dragging { transition: none; }
  @media (max-width: 720px) {
    .toasts { left: 10px; right: 10px; width: auto; bottom: 74px; }
  }
  .toasts.top { top: calc(env(safe-area-inset-top, 0px) + 60px); bottom: auto; flex-direction: column-reverse; }
  .top .toast { border-radius: 20px; padding: 13px 12px 13px 16px; font-size: 13.5px; }
  .top .toast.glass { background: var(--glass-bg); }
</style>
