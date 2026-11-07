<script>
  import { isDesktopApp } from '../lib/platform.js';
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { tick } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { ui, toast } from '../lib/state.svelte.js';
  import { clock } from '../lib/format.js';

  let level = $state('all');
  let search = $state('');
  let box;
  let follow = $state(true);

  const shown = $derived(
    ui.logs
      .filter((l) => (level === 'all' || l.level === level) && (!search || l.msg.toLowerCase().includes(search.toLowerCase())))
      .slice(-800)
  );
  const counts = $derived({
    all: ui.logs.length,
    info: ui.logs.filter((l) => l.level === 'info').length,
    warn: ui.logs.filter((l) => l.level === 'warn').length,
    error: ui.logs.filter((l) => l.level === 'error').length,
  });

  $effect(() => {
    shown.length;
    if (follow && box) tick().then(() => (box.scrollTop = box.scrollHeight));
  });

  function onScroll() {
    follow = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  }

  async function copy() {
    const text = shown.map((l) => `${new Date(l.ts).toISOString()} [${l.level.toUpperCase()}] ${l.msg}`).join('\n');
    await navigator.clipboard.writeText(text);
    toast('success', t('logs.copied'), 2000);
  }

  async function clear() {
    await invoke('clear_logs');
    ui.logs = [];
  }
</script>

<div class="view">
  <header class="head">
    <div>
      <h1>{t('nav.logs')}</h1>
      <p class="subtitle">{t('logs.subtitle')}</p>
    </div>
    <div class="actions">
      <button class="btn sm" onclick={copy}><Icon name="copy" size={14} /> {t('logs.copy')}</button>
      {#if isDesktopApp}<button class="btn sm" onclick={() => invoke('open_path', { path: ui.info.logDir })}><Icon name="folder" size={14} /> {t('logs.openFolder')}</button>{/if}
      <button class="btn ghost sm" onclick={clear}><Icon name="trash" size={14} /> {t('common.clear')}</button>
    </div>
  </header>

  <div class="toolbar">
    {#each [['all', t('logs.all')], ['info', t('logs.info')], ['warn', t('logs.warnings')], ['error', t('logs.errors')]] as [k, label]}
      <button class="fchip {k}" class:sel={level === k} onclick={() => (level = k)}>{label} <span>{counts[k]}</span></button>
    {/each}
    <div class="spacer"></div>
    <div class="search">
      <Icon name="search" size={14} />
      <input class="input" placeholder={t('logs.filter')} bind:value={search} />
    </div>
  </div>

  <div class="log card selectable" bind:this={box} onscroll={onScroll}>
    {#each shown as l, i (l.ts + ':' + i)}
      <div class="line {l.level}">
        <span class="ts">{clock(l.ts)}</span>
        <span class="lvl">{t(`logs.level.${l.level}`)}</span>
        <span class="msg">{l.msg}</span>
      </div>
    {:else}
      <div class="empty">{t('logs.empty')}</div>
    {/each}
  </div>
  {#if !follow}
    <button class="btn sm jump" onclick={() => { follow = true; box.scrollTop = box.scrollHeight; }}><Icon name="arrow-down" size={14} /> {t('logs.jump')}</button>
  {/if}
</div>

<style>
  .view { padding: 26px 30px 24px; height: 100%; display: flex; flex-direction: column; max-width: 1240px; margin: 0 auto; position: relative; }
  .head { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; }
  h1 { margin: 0; font-family: var(--font-display); font-size: 26px; font-weight: 700; }
  .subtitle { margin: 4px 0 0; color: var(--text-2); }
  .actions { display: flex; gap: 8px; }
  .toolbar { display: flex; align-items: center; gap: 8px; margin: 18px 0 12px; }
  .fchip { height: 30px; padding: 0 12px; border-radius: 99px; border: 1px solid var(--border-2); background: var(--panel); color: var(--text-2); cursor: pointer; font-size: 12.5px; font-weight: 600; display: flex; align-items: center; gap: 7px; transition: all 0.2s; }
  .fchip span { font-size: 11px; opacity: 0.7; }
  .fchip.sel { background: rgba(var(--accent-rgb), 0.16); border-color: rgba(var(--accent-rgb), 0.4); color: var(--text); }
  .fchip.warn.sel { background: rgba(251, 191, 36, 0.14); border-color: rgba(251, 191, 36, 0.4); }
  .fchip.error.sel { background: rgba(248, 113, 113, 0.14); border-color: rgba(248, 113, 113, 0.4); }
  .search { position: relative; width: 220px; }
  .search :global(svg) { position: absolute; left: 11px; top: 10px; color: var(--muted); }
  .search .input { height: 32px; padding-left: 32px; font-size: 13px; }
  .log { flex: 1; overflow-y: auto; padding: 8px 0; font-family: var(--mono); font-size: 12.5px; min-height: 0; }
  .line { display: grid; grid-template-columns: 74px 52px 1fr; gap: 10px; padding: 3px 16px; line-height: 1.5; }
  .line:hover { background: rgba(255, 255, 255, 0.025); }
  .ts { color: var(--muted); }
  .lvl { font-weight: 700; font-size: 11px; padding-top: 1px; color: #7dd3fc; }
  .warn .lvl { color: var(--warn); }
  .error .lvl { color: var(--err); }
  .error .msg { color: #fecaca; }
  .msg { word-break: break-word; color: var(--text-2); }
  .empty { padding: 30px; text-align: center; color: var(--muted); font-family: var(--font); }
  .jump { position: absolute; right: 50px; bottom: 44px; box-shadow: var(--shadow); }

  @media (max-width: 720px) {
    .view { padding: 16px 14px 14px; }
    .head { flex-wrap: wrap; gap: 12px; }
    h1 { font-size: 23px; }
    .toolbar { flex-wrap: wrap; margin: 12px 0 10px; }
    .search { width: 100%; order: 5; }
    .search .input { height: 40px; }
    .search :global(svg) { top: 12px; }
    .line { grid-template-columns: 60px 1fr; padding: 4px 10px; font-size: 11.5px; }
    .msg { grid-column: 1 / -1; }
    .jump { right: 18px; bottom: 24px; }
  }
</style>
