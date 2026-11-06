<script>
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { fly, fade } from 'svelte/transition';
  import { flip } from 'svelte/animate';
  import { cubicOut } from 'svelte/easing';
  import Icon from '../components/Icon.svelte';
  import Modal from '../components/Modal.svelte';
  import IndexerForm from '../components/IndexerForm.svelte';
  import Switch from '../components/Switch.svelte';
  import { ui, saveSettings, ask, toast, d } from '../lib/state.svelte.js';
  import { blankIndexer } from '../lib/search.js';
  import { isAndroid } from '../lib/platform.js';
  import Rss from './Rss.svelte';
  import DiscoverIndexers from '../components/DiscoverIndexers.svelte';

  // En el escritorio, RSS es una pestaña de Indexadores (en Android está en Ajustes).
  let tab = $state('indexers');

  let editing = $state(null);
  let isNew = $state(false);
  let testing = $state({});

  const list = $derived(ui.settings?.indexers ?? []);

  function add() {
    editing = blankIndexer();
    isNew = true;
  }

  function edit(ix) {
    editing = structuredClone($state.snapshot(ix));
    isNew = false;
  }

  async function save() {
    const item = $state.snapshot(editing);
    item.url = item.url.trim().replace(/\/+$/, '');
    if (!/^https?:\/\//i.test(item.url)) {
      toast('error', t('indexers.urlInvalid'));
      return;
    }
    if (!item.name.trim()) item.name = item.url.replace(/^https?:\/\/(api\.)?/i, '').split(/[/.]/)[0];
    const all = $state.snapshot(ui.settings.indexers);
    const i = all.findIndex((x) => x.id === item.id);
    if (i >= 0) all[i] = item;
    else all.push(item);
    if (await saveSettings({ indexers: all })) {
      toast('success', isNew ? t('indexers.added') : t('indexers.updated'), 2500);
      editing = null;
    }
  }

  async function remove(ix) {
    const r = await ask({ title: t('indexers.removeTitle'), message: t('indexers.removeMessage', { name: ix.name || ix.url }), confirmLabel: t('common.remove'), danger: true, icon: 'trash' });
    if (!r.ok) return;
    await saveSettings({ indexers: $state.snapshot(ui.settings.indexers).filter((x) => x.id !== ix.id) });
  }

  async function toggle(ix, v) {
    await saveSettings({ indexers: $state.snapshot(ui.settings.indexers).map((x) => (x.id === ix.id ? { ...x, enabled: v } : x)) });
  }

  async function move(i, delta) {
    const all = $state.snapshot(ui.settings.indexers);
    const j = i + delta;
    if (j < 0 || j >= all.length) return;
    [all[i], all[j]] = [all[j], all[i]];
    await saveSettings({ indexers: all });
  }

  async function test(ix) {
    testing[ix.id] = { busy: true };
    try {
      testing[ix.id] = await invoke('test_indexer', { indexer: $state.snapshot(ix) });
    } catch (e) {
      testing[ix.id] = { ok: false, message: String(e) };
    }
  }

  function host(url) {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  }
</script>

{#if !isAndroid}
  <nav class="tabs">
    <button class:sel={tab === 'indexers'} onclick={() => (tab = 'indexers')}><Icon name="layers" size={15} /> {t('nav.indexers')}</button>
    <button class:sel={tab === 'rss'} onclick={() => (tab = 'rss')}><Icon name="rss" size={15} /> RSS</button>
  </nav>
{/if}

{#if tab === 'rss' && !isAndroid}
  <Rss />
{:else}
<div class="view">
  <header class="head">
    <div>
      <h1>{t('nav.indexers')}</h1>
      <p class="subtitle">{t('indexers.subtitle')}</p>
    </div>
    <div class="head-btns">
      <button class="btn" onclick={() => (ui.discoverOpen = true)}><Icon name="download" size={16} /> {t('discover.button')}</button>
      <button class="btn primary" onclick={add}><Icon name="plus" size={16} /> {t('indexers.add')}</button>
    </div>
  </header>

  <div class="list">
    {#each list as ix, i (ix.id)}
      {@const tst = testing[ix.id]}
      <article class="ix card" class:off={!ix.enabled} animate:flip={{ duration: d(350), easing: cubicOut }} in:fly={{ y: 14, duration: d(300) }} out:fade={{ duration: d(180) }}>
        <div class="ix-top">
          <div class="ix-icon" class:first={i === 0 && ix.enabled}><span>{(ix.name || host(ix.url)).slice(0, 2).toUpperCase()}</span></div>
          <div class="ix-main">
            <div class="ix-name">
              {ix.name || host(ix.url)}
              {#if i === 0}<span class="chip accent">{t('common.preferred')}</span>{:else}<span class="chip muted">{t('common.priorityN', { n: i + 1 })}</span>{/if}
              {#if !ix.apiKey}<span class="chip warn">{t('common.noApiKey')}</span>{/if}
            </div>
            <div class="ix-sub">{host(ix.url)}{ix.apiPath && ix.apiPath !== '/api' ? ` · ${ix.apiPath}` : ''}</div>
          </div>
          <div class="ix-actions">
            <button class="icon-btn" title={t('common.priorityUp')} onclick={() => move(i, -1)} disabled={i === 0}><Icon name="chevron-up" size={17} /></button>
            <button class="icon-btn" title={t('common.priorityDown')} onclick={() => move(i, 1)} disabled={i === list.length - 1}><Icon name="chevron-down" size={17} /></button>
            <button class="icon-btn" title={t('common.test')} onclick={() => test(ix)}><Icon name="plug" size={16} /></button>
            <button class="icon-btn" title={t('common.edit')} onclick={() => edit(ix)}><Icon name="edit" size={16} /></button>
            <button class="icon-btn danger" title={t('common.remove')} onclick={() => remove(ix)}><Icon name="trash" size={16} /></button>
            <Switch checked={ix.enabled} onchange={(v) => toggle(ix, v)} label={t('common.enabled')} />
          </div>
        </div>
        {#if tst}
          <div class="alert {tst.busy ? 'info' : tst.ok ? 'ok' : 'err'}" in:fly={{ y: -6, duration: d(200) }}>
            <Icon name={tst.busy ? 'loader' : tst.ok ? 'check-circle' : 'x-circle'} size={15} class={tst.busy ? 'spin' : ''} />
            <span class="selectable">{tst.busy ? t('indexers.testing') : `${tst.message}${tst.ok ? ` · ${tst.latencyMs} ms` : ''}`}</span>
          </div>
        {/if}
      </article>
    {:else}
      <div class="empty card" in:fade>
        <Icon name="layers" size={36} stroke={1.6} />
        <h3>{t('indexers.emptyTitle')}</h3>
        <p>{t('indexers.emptyText')}</p>
        <button class="btn primary" onclick={add}><Icon name="plus" size={16} /> {t('indexers.add')}</button>
      </div>
    {/each}
  </div>

  {#if list.length}
    <div class="alert info tip">
      <Icon name="info" size={16} />
      <span>
        {ui.settings.searchDedup ? t('indexers.dedupOn') : t('indexers.dedupOff')}
        <button class="link" onclick={() => (ui.view = 'settings')}>{t('common.change')}</button>
      </span>
    </div>
  {/if}
</div>
{/if}

{#if editing}
  <Modal title={isNew ? t('indexers.new') : t('indexers.edit')} subtitle={t('indexers.modalSubtitle')} icon="layers" width={640} onclose={() => (editing = null)}>
    <IndexerForm bind:indexer={editing} />
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (editing = null)}>{t('common.cancel')}</button>
      <button class="btn primary" onclick={save}><Icon name="check" size={16} /> {t('common.save')}</button>
    {/snippet}
  </Modal>
{/if}

{#if ui.discoverOpen}
  <DiscoverIndexers onclose={() => (ui.discoverOpen = false)} />
{/if}

<style>
  .head-btns { display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; }
  .tabs { display: flex; gap: 4px; max-width: 1100px; margin: 0 auto; padding: 18px 30px 0; }
  .tabs button { display: inline-flex; align-items: center; gap: 7px; padding: 7px 14px; border-radius: 10px; border: 1px solid transparent; background: none; color: var(--text-2); font-weight: 600; font-size: 13.5px; cursor: pointer; }
  .tabs button:hover { color: var(--text); background: var(--panel-2); }
  .tabs button.sel { color: var(--text); background: rgba(var(--accent-rgb), 0.14); border-color: rgba(var(--accent-rgb), 0.35); }
  .tabs + :global(.view) { padding-top: 14px; }
  .view { padding: 26px 30px 40px; max-width: 1100px; margin: 0 auto; }
  .head { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; }
  h1 { margin: 0; font-family: var(--font-display); font-size: 26px; font-weight: 700; }
  .subtitle { margin: 4px 0 0; color: var(--text-2); }
  .list { display: flex; flex-direction: column; gap: 12px; margin-top: 22px; }
  .ix { padding: 16px 18px; display: flex; flex-direction: column; gap: 12px; transition: opacity 0.3s; }
  .ix.off { opacity: 0.6; }
  .ix-top { display: flex; align-items: center; gap: 14px; }
  .ix-icon { width: 44px; height: 44px; border-radius: 12px; display: grid; place-items: center; background: var(--panel-3); color: var(--text-2); font-weight: 750; font-size: 14px; flex-shrink: 0; letter-spacing: 0.02em; }
  .ix-icon.first { background: var(--grad-diag); color: #fff; box-shadow: 0 8px 22px -10px rgba(var(--accent-rgb), 0.9); }
  .ix-main { flex: 1; min-width: 0; }
  .ix-name { display: flex; align-items: center; gap: 8px; font-weight: 620; font-size: 15px; flex-wrap: wrap; }
  .ix-sub { color: var(--text-2); font-size: 12.5px; margin-top: 3px; }
  .ix-actions { display: flex; align-items: center; gap: 2px; }
  .ix-actions :global(.switch) { margin-left: 8px; }
  .empty { padding: 48px 20px; text-align: center; color: var(--text-2); display: flex; flex-direction: column; align-items: center; gap: 8px; }
  .empty h3 { margin: 8px 0 0; color: var(--text); font-family: var(--font-display); }
  .empty p { margin: 0 0 12px; }
  .tip { margin-top: 16px; }
  .link { border: none; background: none; color: var(--accent-2); cursor: pointer; font-weight: 600; padding: 0; margin-left: 4px; }

  @media (max-width: 720px) {
    .view { padding: 16px 14px 28px; }
    .head { flex-direction: column; align-items: stretch; gap: 12px; }
    h1 { font-size: 23px; }
    .subtitle { font-size: 13px; }
    .list { margin-top: 16px; gap: 10px; }
    .ix { padding: 14px; }
    .ix-top { flex-wrap: wrap; gap: 10px 12px; }
    .ix-icon { width: 40px; height: 40px; }
    .ix-sub { word-break: break-all; }
    .ix-actions { width: 100%; justify-content: flex-end; padding-top: 8px; border-top: 1px solid var(--border); }
    .empty { padding: 32px 12px; }
  }
</style>
