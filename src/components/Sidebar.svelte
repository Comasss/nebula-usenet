<script>
  import { t, tn } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import Icon from './Icon.svelte';
  import SpeedGraph from './SpeedGraph.svelte';
  import { ui } from '../lib/state.svelte.js';
  import { upd } from '../lib/updater.svelte.js';
  import { speed, bytes } from '../lib/format.js';
  import { usableDebrid, debridLabel } from '../lib/debrid.js';

  const ALL = [
    { id: 'search', icon: 'search', feature: 'indexers' },
    { id: 'downloads', icon: 'download', feature: 'nntp' },
    { id: 'history', icon: 'history', feature: 'nntp' },
    { id: 'stats', icon: 'chart' },
    { id: 'indexers', icon: 'layers', feature: 'indexers' },
    { id: 'servers', icon: 'server', feature: 'nntp' },
    { id: 'debrid', icon: 'cloud', feature: 'nntp' },
    { id: 'settings', icon: 'settings' },
    { id: 'logs', icon: 'logs' },
  ];
  const items = $derived(
    ALL.filter(
      (i) => !i.feature || (i.feature === 'nntp' ? ui.settings?.featureNntp !== false : ui.settings?.featureIndexers)
    )
  );
  const ITEM_H = 42;
  const GAP = 4;

  const active = $derived(Math.max(0, items.findIndex((i) => i.id === ui.view)));
  const queueCount = $derived(ui.queue.jobs.length);
  const failedCount = $derived(ui.history.filter((h) => h.status === 'failed').length);
  const errorLogs = $derived(ui.logs.filter((l) => l.level === 'error').length);
  const debridJob = $derived(ui.queue.jobs.find((j) => j.debrid && j.status === 'downloading'));
  const serverState = $derived.by(() => {
    const s = ui.queue.servers;
    const debrid = usableDebrid(ui.settings);
    if (debridJob && !ui.queue.activeConns) return { cls: 'active', text: t('sidebar.downloadingVia', { provider: debridJob.debrid.provider }) };
    if (!ui.settings?.servers?.length) return debrid.length ? { cls: 'ok', text: t('sidebar.readyVia', { provider: debridLabel(debrid[0]) }) } : { cls: 'err', text: t('sidebar.noServers') };
    if (!s.length) return { cls: '', text: t('sidebar.serversDisabled') };
    if (s.some((x) => x.state === 'auth')) return { cls: 'err', text: t('sidebar.authError') };
    if (s.every((x) => x.state === 'down')) return { cls: 'err', text: t('sidebar.offline') };
    if (s.some((x) => x.state === 'down' || x.state === 'warn')) return { cls: 'warn', text: t('sidebar.unstable') };
    const conns = s.reduce((a, x) => a + x.connected, 0);
    if (s.some((x) => x.state === 'active')) return { cls: 'active', text: tn('sidebar.activeConns', ui.queue.activeConns) };
    if (conns) return { cls: 'idle', text: tn('sidebar.openConns', conns) };
    return { cls: 'ok', text: t('sidebar.ready') };
  });

  function togglePause() {
    invoke('set_paused', { paused: !ui.queue.paused });
  }
</script>

<nav class="sidebar">
  <div class="nav" style="--h:{ITEM_H}px; --gap:{GAP}px">
    <div class="pill" style="transform: translateY({active * (ITEM_H + GAP)}px)"></div>
    {#each items as it (it.id)}
      <button class="item" data-nav={it.id} class:active={ui.view === it.id} onclick={() => (ui.view = it.id)}>
        <Icon name={it.icon} size={18} />
        <span class="label">{t(`nav.${it.id}`)}</span>
        {#if it.id === 'downloads' && queueCount}
          <span class="badge">{queueCount}</span>
        {:else if it.id === 'history' && failedCount}
          <span class="badge err">{failedCount}</span>
        {:else if it.id === 'logs' && errorLogs}
          <span class="badge err small"></span>
        {:else if it.id === 'settings' && upd.info?.available}
          <span class="badge small upd" title={t('update.availableLabel', { version: upd.info.version })}></span>
        {/if}
      </button>
    {/each}
  </div>

  <div class="grow"></div>

  {#if ui.settings?.featureNntp !== false}
  <div class="speed-card card">
    <div class="sc-head">
      <div>
        <div class="sc-label">{t('sidebar.speed')}</div>
        <div class="sc-speed">{ui.queue.paused ? '—' : speed(ui.queue.speed)}</div>
      </div>
      <button
        class="pause-btn"
        class:paused={ui.queue.paused}
        onclick={togglePause}
        title={ui.queue.paused ? t('sidebar.resumeAll') : t('sidebar.pauseAll')}>
        <Icon name={ui.queue.paused ? 'play' : 'pause'} size={15} stroke={2.4} />
      </button>
    </div>
    <div class="sc-graph">
      <SpeedGraph data={ui.speedHistory.slice(-60)} height={42} id="side" />
    </div>
    <div class="sc-foot">
      <span class="dot {serverState.cls}"></span>
      <span class="truncate">{serverState.text}</span>
      {#if ui.queue.remaining > 0}
        <span class="spacer"></span>
        <span class="muted nowrap">{bytes(ui.queue.remaining)}</span>
      {/if}
    </div>
  </div>
  {/if}
</nav>

<style>
  .sidebar {
    width: 236px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    padding: 14px 12px 14px;
    background: var(--bg);
    border-right: 1px solid rgba(255, 255, 255, 0.035);
  }
  .nav { position: relative; display: flex; flex-direction: column; gap: var(--gap); }
  .pill {
    position: absolute;
    left: 0; right: 0; top: 0;
    height: var(--h);
    border-radius: 11px;
    background: linear-gradient(90deg, rgba(var(--accent-rgb), 0.2), rgba(var(--accent-rgb), 0.06));
    border: 1px solid rgba(var(--accent-rgb), 0.28);
    transition: transform 0.42s var(--ease);
    pointer-events: none;
  }
  .pill::before {
    content: '';
    position: absolute;
    left: -1px; top: 10px; bottom: 10px;
    width: 3px;
    border-radius: 3px;
    background: var(--grad-diag);
    box-shadow: 0 0 12px rgba(var(--accent-rgb), 0.9);
  }
  .item {
    position: relative;
    height: var(--h);
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 14px;
    border: none;
    border-radius: 11px;
    background: transparent;
    color: var(--text-2);
    font-weight: 550;
    cursor: pointer;
    text-align: left;
    transition: color 0.2s, background 0.2s;
  }
  .item:hover:not(.active) { background: rgba(255, 255, 255, 0.035); color: var(--text); }
  .item.active { color: var(--text); }
  .item.active :global(svg) { color: var(--accent-2); }
  .item :global(svg) { transition: color 0.25s, transform 0.25s var(--ease); }
  .item:hover :global(svg) { transform: scale(1.08); }
  .label { flex: 1; }
  .badge {
    min-width: 20px; height: 20px; padding: 0 6px;
    border-radius: 999px;
    display: grid; place-items: center;
    font-size: 11px; font-weight: 700;
    background: rgba(var(--accent-rgb), 0.22);
    color: var(--text);
    animation: badge-in 0.35s var(--ease);
  }
  .badge.err { background: rgba(248, 113, 113, 0.2); color: #fca5a5; }
  .badge.small.upd { background: var(--accent-2); }
  .badge.small { min-width: 8px; width: 8px; height: 8px; padding: 0; background: var(--err); }
  @keyframes badge-in { from { transform: scale(0.4); opacity: 0; } }
  .grow { flex: 1; }

  .speed-card { padding: 14px 14px 12px; overflow: hidden; }
  .sc-head { display: flex; align-items: flex-start; justify-content: space-between; }
  .sc-label { font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); font-weight: 650; }
  .sc-speed { font-family: var(--font-display); font-size: 20px; font-weight: 650; font-variant-numeric: tabular-nums; margin-top: 2px; }
  .pause-btn {
    width: 34px; height: 34px;
    border-radius: 50%;
    border: 1px solid var(--border-2);
    background: var(--panel-2);
    color: var(--text);
    display: grid; place-items: center;
    cursor: pointer;
    transition: all 0.2s var(--ease);
  }
  .pause-btn:hover { background: var(--panel-3); transform: scale(1.06); }
  .pause-btn.paused { background: var(--grad-diag); border-color: transparent; box-shadow: 0 4px 16px -4px rgba(var(--accent-rgb), 0.9); }
  .sc-graph { margin: 10px -14px 8px; }
  .sc-foot { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--text-2); min-width: 0; }
  .nowrap { white-space: nowrap; flex-shrink: 0; }
</style>
