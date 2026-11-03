<script>
  // Panel de velocidad (Windows): apagar, suspender o hibernar al vaciarse la cola, solo esta vez.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { t } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';
  import { ui, toast, d } from '../lib/state.svelte.js';

  let open = $state(false);
  let cd = $state(null);
  const armed = $derived((ui.settings?.finishActions ?? []).find((a) => a.kind === 'power' && a.enabled && a.onQueueEmpty));

  onMount(() => {
    const offs = [];
    invoke('power_status').then((c) => (cd = c)).catch(() => {});
    listen('power-countdown', (e) => (cd = e.payload.remaining > 0 ? e.payload : null)).then((f) => offs.push(f));
    listen('power-cancelled', () => (cd = null)).then((f) => offs.push(f));
    return () => offs.forEach((f) => f());
  });

  async function pick(mode) {
    open = false;
    try {
      ui.settings = await invoke('power_quick', { mode });
      if (mode) toast('info', t('finish.quick.armed', { action: t(`finish.power.${mode}`) }), 3500);
    } catch (e) {
      toast('error', String(e));
    }
  }
</script>

<svelte:window onclick={() => (open = false)} />

<div class="pq">
  {#if cd && !cd.test}
    <button class="pq-btn on" onclick={(e) => { e.stopPropagation(); invoke('power_cancel'); }} title={t('finish.quick.cancel')}>
      <Icon name="x" size={14} /> {t('finish.quick.cancelIn', { secs: cd.remaining })}
    </button>
  {:else}
    <button class="pq-btn" class:on={!!armed} onclick={(e) => { e.stopPropagation(); open = !open; }} title={t('finish.quick.title')} aria-label={t('finish.quick.title')}>
      <Icon name={armed && armed.power !== 'shutdown' ? 'moon' : 'power'} size={14} />
      {#if armed}{t(`finish.power.${armed.power || 'shutdown'}`)}{/if}
    </button>
  {/if}
  {#if open}
    <div class="popover card" transition:scale={{ start: 0.95, duration: d(160), easing: cubicOut }} onclick={(e) => e.stopPropagation()} role="presentation">
      <div class="p-title">{t('finish.quick.title')}</div>
      <button class="pop-item" class:sel={!armed} onclick={() => pick(null)}>
        {t('finish.quick.off')}
        {#if !armed}<Icon name="check" size={14} />{/if}
      </button>
      {#each ['shutdown', 'sleep', 'hibernate'] as m}
        <button class="pop-item" class:sel={armed?.power === m} onclick={() => pick(m)}>
          {t(`finish.power.${m}`)}
          {#if armed?.power === m}<Icon name="check" size={14} />{/if}
        </button>
      {/each}
      <div class="p-note">{armed && !armed.once ? t('finish.quick.always') : t('finish.quick.once')}</div>
    </div>
  {/if}
</div>

<style>
  .pq { position: relative; }
  .pq-btn {
    display: inline-flex; align-items: center; gap: 6px;
    height: 28px; padding: 0 9px;
    border-radius: 8px; border: 1px solid var(--border-2);
    background: var(--panel-2); color: var(--text-2);
    font-size: 12px; font-weight: 600; cursor: pointer; transition: all 0.2s;
  }
  .pq-btn:hover { color: var(--text); background: var(--panel-3); }
  .pq-btn.on { border-color: rgba(248, 113, 113, 0.45); color: #fca5a5; background: rgba(248, 113, 113, 0.08); }
  .popover {
    position: absolute; right: 0; top: 34px; z-index: 30; width: 220px; padding: 6px;
    background: var(--panel-2); box-shadow: var(--shadow); transform-origin: top right;
  }
  .p-title { padding: 6px 10px 4px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); font-weight: 650; }
  .pop-item {
    width: 100%; height: 32px; display: flex; align-items: center; justify-content: space-between;
    padding: 0 10px; border: none; border-radius: 7px; background: transparent; cursor: pointer; font-size: 13px; color: var(--text);
  }
  .pop-item:hover { background: var(--panel-3); }
  .pop-item.sel { color: var(--accent-2); font-weight: 600; }
  .p-note { padding: 6px 10px 4px; border-top: 1px solid var(--border); margin-top: 4px; font-size: 11.5px; color: var(--muted); line-height: 1.4; }
</style>
