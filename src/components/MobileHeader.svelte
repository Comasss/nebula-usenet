<script>
  // Cabecera de la versión móvil: ocupa el lugar de la barra de título y del panel de velocidad
  // de la barra lateral.
  import { t } from '../lib/i18n.svelte.js';
  import { remote, call } from '../lib/remote.svelte.js';
  import Icon from './Icon.svelte';
  import logo from '../assets/logo.svg';
  import { ui } from '../lib/state.svelte.js';
  import { speed } from '../lib/format.js';

  const downloading = $derived(!ui.queue.paused && ui.queue.speed > 0);
  const nntp = $derived(!!remote.active || ui.settings?.featureNntp !== false);
</script>

<header class="mhead">
  <div class="brand">
    <img src={logo} alt="" width="22" height="22" />
    <span>Nebula</span>
  </div>
  <div class="right">
    {#if ui.queue.paused && ui.queue.jobs.length}
      <span class="status paused"><Icon name="pause" size={11} stroke={2.5} /> {t('status.paused')}</span>
    {:else if downloading}
      <span class="status live"><Icon name="arrow-down" size={11} stroke={2.8} /> {speed(ui.queue.speed)}</span>
    {/if}
    {#if nntp && (ui.queue.jobs.length || ui.queue.paused)}
      <button
        class="pause-btn"
        class:paused={ui.queue.paused}
        onclick={() => call('set_paused', { paused: !ui.queue.paused })}
        aria-label={ui.queue.paused ? t('sidebar.resumeAll') : t('sidebar.pauseAll')}>
        <Icon name={ui.queue.paused ? 'play' : 'pause'} size={15} stroke={2.4} />
      </button>
    {/if}
  </div>
</header>

<style>
  .mhead {
    height: 54px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 14px 0 16px;
    background: var(--bg);
    border-bottom: 1px solid rgba(255, 255, 255, 0.035);
    position: relative;
    z-index: 50;
  }
  .brand { display: flex; align-items: center; gap: 10px; }
  .brand span { font-family: var(--font-display); font-weight: 700; font-size: 17px; letter-spacing: 0.01em; }
  .right { display: flex; align-items: center; gap: 10px; }
  .status {
    display: inline-flex; align-items: center; gap: 6px;
    height: 26px; padding: 0 11px; border-radius: 999px;
    font-size: 12.5px; font-weight: 600; font-variant-numeric: tabular-nums;
    animation: pop 0.35s var(--ease);
  }
  .status.live { background: rgba(var(--accent-rgb), 0.14); color: var(--text); }
  .status.live :global(svg) { color: var(--accent-2); }
  .status.paused { background: var(--panel-3); color: var(--text-2); }
  @keyframes pop { from { transform: scale(0.85); opacity: 0; } }
  .pause-btn {
    width: 36px; height: 36px;
    border-radius: 50%;
    border: 1px solid var(--border-2);
    background: var(--panel-2);
    color: var(--text);
    display: grid; place-items: center;
    cursor: pointer;
    transition: all 0.2s var(--ease);
  }
  .pause-btn:active { transform: scale(0.92); }
  .pause-btn.paused { background: var(--grad-diag); border-color: transparent; box-shadow: 0 4px 16px -4px rgba(var(--accent-rgb), 0.9); }
</style>
