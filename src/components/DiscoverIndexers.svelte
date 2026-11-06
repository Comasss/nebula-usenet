<script>
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';
  import Switch from './Switch.svelte';
  import { ui, saveSettings, toast } from '../lib/state.svelte.js';

  // Indexadores › Desde Prowlarr / NZBHydra2: cada indexador Usenet de Prowlarr, o NZBHydra2 como uno solo.
  let { onclose } = $props();

  let url = $state('');
  let apiKey = $state('');
  let showKey = $state(false);
  let busy = $state(false);
  let error = $state('');
  let found = $state(null);
  let picked = $state({});

  const norm = (u) => u.trim().replace(/\/+$/, '').toLowerCase();
  const exists = (ix) => (ui.settings.indexers ?? []).some((x) => norm(x.url) === norm(ix.url));
  const count = $derived(Object.values(picked).filter(Boolean).length);

  async function discover() {
    busy = true;
    error = '';
    found = null;
    try {
      found = await invoke('indexer_discover', { url: url.trim(), apiKey: apiKey.trim() });
      picked = Object.fromEntries(found.map((ix) => [ix.id, !exists(ix)]));
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function add() {
    const items = found.filter((ix) => picked[ix.id]);
    if (await saveSettings({ indexers: [...$state.snapshot(ui.settings.indexers ?? []), ...items] })) {
      toast('success', t('discover.added', { n: items.length }), 3000);
      onclose();
    }
  }
</script>

<Modal title={t('discover.title')} subtitle={t('discover.subtitle')} icon="layers" width={520} {onclose} dismissable={!busy}>
  <div class="disc">
    <div class="field">
      <label for="dc-url">URL</label>
      <input id="dc-url" class="input" placeholder="http://prowlarr:9696" bind:value={url} oninput={() => (found = null)} spellcheck="false" onkeydown={(e) => e.key === 'Enter' && url.trim() && discover()} />
    </div>
    <div class="field">
      <label for="dc-key">{t('common.apiKey')}</label>
      <div class="pw">
        <input id="dc-key" class="input mono" type={showKey ? 'text' : 'password'} bind:value={apiKey} oninput={() => (found = null)} autocomplete="off" spellcheck="false" />
        <button type="button" class="icon-btn eye" onclick={() => (showKey = !showKey)} aria-label={t('common.showApiKey')}><Icon name={showKey ? 'eye-off' : 'eye'} size={16} /></button>
      </div>
      <span class="hint">{t('discover.hint')}</span>
    </div>

    {#if error}<div class="alert err"><Icon name="alert-triangle" size={15} /> <span>{error}</span></div>{/if}

    {#if found}
      <div class="list">
        {#each found as ix (ix.id)}
          <div class="row">
            <div class="main">
              <div class="name">{ix.name}{#if !ix.enabled}<span class="chip muted">{t('import.disabled')}</span>{/if}{#if exists(ix)}<span class="chip warn">{t('import.exists')}</span>{/if}</div>
              <div class="sub">{ix.url}</div>
            </div>
            <Switch checked={picked[ix.id]} disabled={exists(ix)} onchange={(v) => (picked[ix.id] = v)} label={ix.name} />
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#snippet footer()}
    <button class="btn ghost" onclick={onclose} disabled={busy}>{t('common.cancel')}</button>
    {#if found && count}
      <button class="btn primary" onclick={add}><Icon name="plus" size={15} /> {t('discover.add', { n: count })}</button>
    {:else}
      <button class="btn primary" onclick={discover} disabled={busy || !url.trim()}><Icon name="search" size={15} /> {busy ? t('discover.searching') : t('discover.find')}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .disc { display: flex; flex-direction: column; gap: 12px; }
  .field { display: flex; flex-direction: column; gap: 6px; }
  .field label { font-size: 12.5px; color: var(--muted); }
  .pw { position: relative; }
  .pw .input { width: 100%; padding-right: 38px; }
  .eye { position: absolute; right: 6px; top: 50%; transform: translateY(-50%); }
  .list { display: flex; flex-direction: column; gap: 6px; }
  .row { display: flex; align-items: center; gap: 12px; padding: 8px 10px; border-radius: 10px; background: var(--panel-2); }
  .main { flex: 1; min-width: 0; }
  .name { font-size: 13.5px; font-weight: 500; display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .sub { font-size: 12px; color: var(--muted); overflow-wrap: anywhere; }
</style>
