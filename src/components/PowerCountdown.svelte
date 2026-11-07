<script>
  // Ventana pequeña y siempre encima con la cuenta atrás antes de apagar, suspender o hibernar.
  // La abre el motor cuando la cola se queda vacía; al terminar o cancelar, la cierra él mismo.
  import { invoke, listen } from '../lib/transport.js';
    import { onMount } from 'svelte';
  import { t, i18n } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';

  let cd = $state(null);
  let total = $state(60);

  onMount(() => {
    const offs = [];
    invoke('get_settings')
      .then((s) => {
        if (s.language) i18n.lang = s.language;
        document.documentElement.dataset.accent = s.accent || 'nebula';
      })
      .catch(() => {});
    invoke('power_status').then((c) => {
      if (c) {
        cd = c;
        total = Math.max(c.remaining, c.test ? 10 : 60);
      }
    });
    listen('power-countdown', (e) => {
      if (!cd || e.payload.remaining > cd.remaining) total = Math.max(e.payload.remaining, e.payload.test ? 10 : 60);
      cd = e.payload;
    }).then((f) => offs.push(f));
    return () => offs.forEach((f) => f());
  });

  const pct = $derived(cd ? (cd.remaining / total) * 100 : 100);
  const R = 34;
  const C = 2 * Math.PI * R;
</script>

<div class="pw" data-tauri-drag-region>
  <div class="ring" data-tauri-drag-region>
    <svg width="84" height="84" viewBox="0 0 84 84" aria-hidden="true">
      <circle cx="42" cy="42" r={R} class="track" />
      <circle cx="42" cy="42" r={R} class="bar" stroke-dasharray={C} stroke-dashoffset={C * (1 - pct / 100)} />
    </svg>
    <span class="secs">{cd?.remaining ?? '…'}</span>
  </div>
  <div class="body" data-tauri-drag-region>
    <div class="title" data-tauri-drag-region><Icon name={cd?.mode === 'shutdown' || !cd ? 'power' : 'moon'} size={16} /> {t(`finish.countdown.${cd?.mode ?? 'shutdown'}`)}</div>
    <div class="sub" data-tauri-drag-region>{cd?.test ? t('finish.countdown.test') : t('finish.countdown.sub', { secs: cd?.remaining ?? 60 })}</div>
    <button class="btn primary" onclick={() => invoke('power_cancel')}><Icon name="x" size={15} /> {t('common.cancel')}</button>
  </div>
</div>

<style>
  :global(html), :global(body) { background: var(--bg); overflow: hidden; }
  .pw {
    position: fixed; inset: 0; display: flex; align-items: center; gap: 22px; padding: 24px 26px;
    border: 1px solid var(--border-2);
    background: radial-gradient(400px 220px at 0% 0%, rgba(var(--accent-rgb), 0.16), transparent 70%), var(--panel);
    user-select: none;
  }
  .ring { position: relative; width: 84px; height: 84px; flex-shrink: 0; }
  .ring svg { transform: rotate(-90deg); }
  .track { fill: none; stroke: var(--panel-3); stroke-width: 7; }
  .bar { fill: none; stroke: var(--accent); stroke-width: 7; stroke-linecap: round; transition: stroke-dashoffset 1s linear; }
  .secs { position: absolute; inset: 0; display: grid; place-items: center; font-family: var(--font-display); font-size: 26px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .body { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .title { display: flex; align-items: center; gap: 8px; font-size: 16px; font-weight: 650; }
  .title :global(svg) { color: var(--accent-2); }
  .sub { font-size: 12.5px; color: var(--muted); line-height: 1.45; margin-bottom: 8px; }
  .btn { align-self: flex-start; }
</style>
