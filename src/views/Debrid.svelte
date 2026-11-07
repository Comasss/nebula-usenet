<script>
  import { t, th } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { fly, fade } from 'svelte/transition';
  import { flip } from 'svelte/animate';
  import { cubicOut } from 'svelte/easing';
  import Icon from '../components/Icon.svelte';
  import Modal from '../components/Modal.svelte';
  import DebridForm from '../components/DebridForm.svelte';
  import Switch from '../components/Switch.svelte';
  import { ui, saveSettings, ask, toast, d } from '../lib/state.svelte.js';
  import { blankDebrid, debridLabel, kindInfo, usableDebrid, hasNntp } from '../lib/debrid.js';

  let editing = $state(null);
  let isNew = $state(false);
  let testing = $state({});

  const list = $derived(ui.settings?.debrid ?? []);
  const usable = $derived(usableDebrid(ui.settings));
  const byDefault = $derived(usable.length > 0 && (ui.settings?.downloadVia === 'debrid' || !hasNntp(ui.settings)));

  function add() {
    editing = blankDebrid(list.some((p) => p.kind === 'torbox') ? 'premiumize' : 'torbox');
    isNew = true;
  }

  function edit(p) {
    editing = structuredClone($state.snapshot(p));
    isNew = false;
  }

  async function save() {
    const item = $state.snapshot(editing);
    item.apiKey = item.apiKey.trim();
    item.name = item.name.trim();
    if (!item.apiKey) {
      toast('error', t('debrid.pasteKey'));
      return;
    }
    const all = $state.snapshot(ui.settings.debrid ?? []);
    const i = all.findIndex((x) => x.id === item.id);
    if (i >= 0) all[i] = item;
    else all.push(item);
    if (await saveSettings({ debrid: all })) {
      toast('success', isNew ? t('debridView.added', { name: debridLabel(item) }) : t('debridView.updated', { name: debridLabel(item) }), 2500);
      editing = null;
    }
  }

  async function remove(p) {
    const r = await ask({ title: t('debridView.removeTitle'), message: t('common.removeMessage', { name: debridLabel(p) }), confirmLabel: t('common.remove'), danger: true, icon: 'trash' });
    if (!r.ok) return;
    await saveSettings({ debrid: $state.snapshot(ui.settings.debrid).filter((x) => x.id !== p.id) });
  }

  async function toggle(p, v) {
    await saveSettings({ debrid: $state.snapshot(ui.settings.debrid).map((x) => (x.id === p.id ? { ...x, enabled: v } : x)) });
  }

  async function move(i, delta) {
    const all = $state.snapshot(ui.settings.debrid);
    const j = i + delta;
    if (j < 0 || j >= all.length) return;
    [all[i], all[j]] = [all[j], all[i]];
    await saveSettings({ debrid: all });
  }

  async function test(p) {
    testing[p.id] = { busy: true };
    try {
      testing[p.id] = await invoke('test_debrid', { provider: $state.snapshot(p) });
    } catch (e) {
      testing[p.id] = { ok: false, message: String(e) };
    }
  }

  function masked(key) {
    const k = (key ?? '').trim();
    return k.length > 8 ? `${k.slice(0, 4)}…${k.slice(-4)}` : k ? '••••' : t('debridView.noKey');
  }
</script>

<div class="view">
  <header class="head">
    <div>
      <h1>{t('nav.debrid')}</h1>
      <p class="subtitle">{t('debridView.subtitle')}</p>
    </div>
    <button class="btn primary" onclick={add}><Icon name="plus" size={16} /> {t('debridView.add')}</button>
  </header>

  <div class="list">
    {#each list as p, i (p.id)}
      {@const tst = testing[p.id]}
      {@const info = kindInfo(p.kind)}
      {@const first = usable[0]?.id === p.id}
      <article class="db card" class:off={!p.enabled} animate:flip={{ duration: d(350), easing: cubicOut }} in:fly={{ y: 14, duration: d(300) }} out:fade={{ duration: d(180) }}>
        <div class="db-top">
          <div class="db-icon" class:first><Icon name="cloud" size={20} /></div>
          <div class="db-main">
            <div class="db-name">
              {debridLabel(p)}
              {#if first}<span class="chip accent">{t('common.preferred')}</span>{:else}<span class="chip muted">{t('common.priorityN', { n: i + 1 })}</span>{/if}
              {#if !p.apiKey?.trim()}<span class="chip warn">{t('common.noApiKey')}</span>{/if}
            </div>
            <div class="db-sub">{#if p.name?.trim()}{info.name} · {/if}{t('common.apiKey')} <span class="mono">{masked(p.apiKey)}</span></div>
          </div>
          <div class="db-actions">
            <button class="icon-btn" title={t('common.priorityUp')} onclick={() => move(i, -1)} disabled={i === 0}><Icon name="chevron-up" size={17} /></button>
            <button class="icon-btn" title={t('common.priorityDown')} onclick={() => move(i, 1)} disabled={i === list.length - 1}><Icon name="chevron-down" size={17} /></button>
            <button class="icon-btn" title={t('common.test')} onclick={() => test(p)}><Icon name="plug" size={16} /></button>
            <button class="icon-btn" title={t('common.edit')} onclick={() => edit(p)}><Icon name="edit" size={16} /></button>
            <button class="icon-btn danger" title={t('common.remove')} onclick={() => remove(p)}><Icon name="trash" size={16} /></button>
            <Switch checked={p.enabled} onchange={(v) => toggle(p, v)} label={t('common.enabled')} />
          </div>
        </div>
        {#if tst}
          <div class="alert {tst.busy ? 'info' : tst.ok ? 'ok' : 'err'}" in:fly={{ y: -6, duration: d(200) }}>
            <Icon name={tst.busy ? 'loader' : tst.ok ? 'check-circle' : 'x-circle'} size={15} class={tst.busy ? 'spin' : ''} />
            <span class="selectable">{tst.busy ? t('debridView.checking') : `${tst.message}${tst.ok ? ` · ${tst.latencyMs} ms` : ''}`}</span>
          </div>
          {#if tst.warning}
            <div class="alert warn"><Icon name="alert-triangle" size={15} /><span>{tst.warning}</span></div>
          {/if}
        {/if}
      </article>
    {:else}
      <div class="empty card" in:fade>
        <Icon name="cloud" size={36} stroke={1.6} />
        <h3>{t('debridView.emptyTitle')}</h3>
        <p>{@html th('debridView.emptyText')}</p>
        <button class="btn primary" onclick={add}><Icon name="plus" size={16} /> {t('debridView.add')}</button>
      </div>
    {/each}
  </div>

  {#if list.length}
    <div class="alert info tip">
      <Icon name="info" size={16} />
      <span>
        {#if byDefault}
          {@html th('debridView.defaultDebrid', { name: debridLabel(usable[0]) })}{#if usable.length > 1}{t('debridView.fallbackNext')}{/if}{#if ui.settings.debridFallbackNntp && hasNntp(ui.settings)}{t('debridView.fallbackNntp')}{/if}{t('debridView.end')}
        {:else}
          {t('debridView.defaultNntp')}
        {/if}
        <button class="link" onclick={() => (ui.view = 'settings')}>{t('common.change')}</button>
      </span>
    </div>
  {/if}
</div>

{#if editing}
  <Modal title={isNew ? t('debridView.new') : t('debridView.edit')} subtitle={t('debridView.modalSubtitle')} icon="cloud" width={640} onclose={() => (editing = null)}>
    <DebridForm bind:provider={editing} />
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (editing = null)}>{t('common.cancel')}</button>
      <button class="btn primary" onclick={save}><Icon name="check" size={16} /> {t('common.save')}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .view { padding: 26px 30px 40px; max-width: 1100px; margin: 0 auto; }
  .head { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; }
  h1 { margin: 0; font-family: var(--font-display); font-size: 26px; font-weight: 700; }
  .subtitle { margin: 4px 0 0; color: var(--text-2); max-width: 720px; }
  .list { display: flex; flex-direction: column; gap: 12px; margin-top: 22px; }
  .db { padding: 16px 18px; display: flex; flex-direction: column; gap: 12px; transition: opacity 0.3s; }
  .db.off { opacity: 0.6; }
  .db-top { display: flex; align-items: center; gap: 14px; }
  .db-icon { width: 44px; height: 44px; border-radius: 12px; display: grid; place-items: center; background: var(--panel-3); color: var(--text-2); flex-shrink: 0; }
  .db-icon.first { background: var(--grad-diag); color: #fff; box-shadow: 0 8px 22px -10px rgba(var(--accent-rgb), 0.9); }
  .db-main { flex: 1; min-width: 0; }
  .db-name { display: flex; align-items: center; gap: 8px; font-weight: 620; font-size: 15px; flex-wrap: wrap; }
  .db-sub { color: var(--text-2); font-size: 12.5px; margin-top: 3px; }
  .db-actions { display: flex; align-items: center; gap: 2px; }
  .db-actions :global(.switch) { margin-left: 8px; }
  .empty { padding: 48px 20px; text-align: center; color: var(--text-2); display: flex; flex-direction: column; align-items: center; gap: 8px; }
  .empty h3 { margin: 8px 0 0; color: var(--text); font-family: var(--font-display); }
  .empty p { margin: 0 0 12px; max-width: 520px; }
  .tip { margin-top: 16px; }
  .link { border: none; background: none; color: var(--accent-2); cursor: pointer; font-weight: 600; padding: 0; margin-left: 4px; }

  @media (max-width: 720px) {
    .view { padding: 16px 14px 28px; }
    .head { flex-direction: column; align-items: stretch; gap: 12px; }
    h1 { font-size: 23px; }
    .subtitle { font-size: 13px; }
    .list { margin-top: 16px; gap: 10px; }
    .db { padding: 14px; }
    .db-top { flex-wrap: wrap; gap: 10px 12px; }
    .db-icon { width: 40px; height: 40px; }
    .db-sub { word-break: break-all; }
    .db-actions { width: 100%; justify-content: flex-end; padding-top: 8px; border-top: 1px solid var(--border); }
    .empty { padding: 32px 12px; }
  }
</style>
