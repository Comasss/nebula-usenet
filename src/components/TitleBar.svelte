<script>
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Icon from './Icon.svelte';
  import logo from '../assets/logo.svg';
  import { ui } from '../lib/state.svelte.js';
  import { speed } from '../lib/format.js';

  const win = getCurrentWindow();
  let maximized = $state(false);

  $effect(() => {
    let unlisten;
    win.isMaximized().then((v) => (maximized = v));
    win
      .onResized(async () => {
        maximized = await win.isMaximized();
      })
      .then((f) => (unlisten = f));
    return () => unlisten?.();
  });

  const downloading = $derived(!ui.queue.paused && ui.queue.speed > 0);
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand">
    <img src={logo} alt="" width="20" height="20" />
    <span>Nebula</span>
  </div>
  <div class="center">
    {#if ui.queue.paused && ui.queue.jobs.length}
      <span class="status paused"><Icon name="pause" size={11} stroke={2.5} /> {t('status.paused')}</span>
    {:else if downloading}
      <span class="status live"><Icon name="arrow-down" size={11} stroke={2.8} /> {speed(ui.queue.speed)}</span>
    {/if}
  </div>
  <div class="controls">
    <button class="tb" onclick={() => invoke('window_minimize')} title={t('window.minimize')} aria-label={t('window.minimize')}>
      <Icon name="minus" size={16} stroke={1.6} />
    </button>
    <button class="tb" onclick={() => win.toggleMaximize()} title={maximized ? t('window.restore') : t('window.maximize')} aria-label={t('window.maximize')}>
      <Icon name={maximized ? 'restore' : 'maximize'} size={13} stroke={1.6} />
    </button>
    <button class="tb close" onclick={() => invoke('window_close')} title={t('common.close')} aria-label={t('common.close')}>
      <Icon name="x" size={16} stroke={1.6} />
    </button>
  </div>
</header>

<style>
  .titlebar {
    height: 40px;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    padding-left: 14px;
    background: var(--bg);
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    flex-shrink: 0;
    position: relative;
    z-index: 50;
  }
  .brand, .center { pointer-events: none; display: flex; align-items: center; gap: 9px; }
  .brand span { font-family: var(--font-display); font-weight: 650; font-size: 13.5px; letter-spacing: 0.02em; color: var(--text-2); }
  .center { justify-content: center; }
  .status {
    display: inline-flex; align-items: center; gap: 6px;
    height: 22px; padding: 0 10px; border-radius: 999px;
    font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums;
    animation: pop 0.35s var(--ease);
  }
  .status.live { background: rgba(var(--accent-rgb), 0.14); color: var(--text); }
  .status.live :global(svg) { color: var(--accent-2); }
  .status.paused { background: var(--panel-3); color: var(--text-2); }
  @keyframes pop { from { transform: scale(0.85); opacity: 0; } }
  .controls { display: flex; justify-self: end; height: 100%; }
  .tb {
    width: 46px; height: 100%;
    display: grid; place-items: center;
    border: none; background: transparent; color: var(--text-2);
    cursor: pointer; transition: background 0.15s, color 0.15s;
  }
  .tb:hover { background: rgba(255, 255, 255, 0.07); color: var(--text); }
  .tb.close:hover { background: #e81123; color: #fff; }
</style>
