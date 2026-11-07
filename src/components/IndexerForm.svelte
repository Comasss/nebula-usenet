<script>
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { fly } from 'svelte/transition';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import { d } from '../lib/state.svelte.js';
  import { PRESETS } from '../lib/search.js';

  let { indexer = $bindable(), compact = false } = $props();
  let showKey = $state(false);
  let testing = $state(false);
  let result = $state(null);
  let advanced = $state(false);

  const preset = $derived(PRESETS.find((p) => p.url === indexer.url.trim().replace(/\/+$/, ''))?.name ?? '');

  function pickPreset(name) {
    const p = PRESETS.find((x) => x.name === name);
    if (!p) return;
    indexer.url = p.url;
    if (!indexer.name.trim() || PRESETS.some((x) => x.name === indexer.name)) indexer.name = p.name;
    result = null;
  }

  async function test() {
    if (!indexer.url.trim()) {
      result = { ok: false, message: t('indexer.urlRequired') };
      return;
    }
    testing = true;
    result = null;
    try {
      result = await invoke('test_indexer', { indexer: $state.snapshot(indexer) });
    } catch (e) {
      result = { ok: false, message: String(e) };
    } finally {
      testing = false;
    }
  }
</script>

<div class="ixf">
  <div class="field">
    <span class="label">{t('indexer.indexer')}</span>
    <div class="presets">
      {#each PRESETS as p}
        <button type="button" class="preset" class:sel={preset === p.name} onclick={() => pickPreset(p.name)}>{p.name}</button>
      {/each}
      <button type="button" class="preset" class:sel={!preset} onclick={() => { indexer.url = ''; indexer.name = ''; }}>{t('indexer.other')}</button>
    </div>
  </div>
  <div class="grid">
    <div class="field span2">
      <label for="ix-url">URL</label>
      <input id="ix-url" class="input" placeholder={t('indexer.urlPlaceholder')} bind:value={indexer.url} spellcheck="false" />
    </div>
    <div class="field">
      <label for="ix-name">{t('common.name')}</label>
      <input id="ix-name" class="input" placeholder={t('indexer.namePlaceholder')} bind:value={indexer.name} />
    </div>
    <div class="field span3">
      <label for="ix-key">{t('common.apiKey')}</label>
      <div class="pw">
        <input id="ix-key" class="input mono" type={showKey ? 'text' : 'password'} bind:value={indexer.apiKey} autocomplete="off" spellcheck="false" placeholder={t('indexer.keyPlaceholder')} />
        <button type="button" class="icon-btn eye" onclick={() => (showKey = !showKey)} aria-label={t('common.showApiKey')}><Icon name={showKey ? 'eye-off' : 'eye'} size={16} /></button>
      </div>
    </div>
    {#if advanced}
      <div class="field" transition:fly={{ y: -6, duration: d(180) }}>
        <label for="ix-path">{t('indexer.apiPath')}</label>
        <input id="ix-path" class="input mono" bind:value={indexer.apiPath} placeholder="/api" />
      </div>
    {/if}
  </div>

  <div class="row2">
    {#if !compact}
      <label class="opt"><Switch checked={indexer.enabled} onchange={(v) => (indexer.enabled = v)} /> {t('common.enabled')}</label>
    {/if}
    <button type="button" class="btn ghost sm" onclick={() => (advanced = !advanced)}>
      <Icon name="settings" size={13} /> {advanced ? t('indexer.hideAdvanced') : t('indexer.advanced')}
    </button>
  </div>

  <div class="test-row">
    <button type="button" class="btn" onclick={test} disabled={testing}>
      {#if testing}<Icon name="loader" size={15} class="spin" /> {t('common.testing')}{:else}<Icon name="plug" size={15} /> {t('indexer.test')}{/if}
    </button>
    {#if result}
      <div class="result {result.ok ? 'ok' : 'err'}" in:fly={{ x: -10, duration: d(250) }}>
        <Icon name={result.ok ? 'check-circle' : 'x-circle'} size={16} />
        <span class="selectable">{result.message}{#if result.ok && result.latencyMs}{' · '}{result.latencyMs} ms{/if}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .ixf { display: flex; flex-direction: column; gap: 16px; }
  .presets { display: flex; flex-wrap: wrap; gap: 6px; }
  .preset {
    height: 30px; padding: 0 12px; border-radius: 8px;
    border: 1px solid var(--border-2); background: var(--panel-2);
    color: var(--text-2); font-size: 12.5px; font-weight: 600; cursor: pointer;
    transition: all 0.15s;
  }
  .preset:hover { color: var(--text); background: var(--panel-3); }
  .preset.sel { border-color: rgba(var(--accent-rgb), 0.6); background: rgba(var(--accent-rgb), 0.16); color: var(--text); }
  .grid { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 14px; }
  .span2 { grid-column: span 2; }
  .span3 { grid-column: span 3; }
  .pw { position: relative; }
  .pw .input { padding-right: 40px; }
  .eye { position: absolute; right: 3px; top: 3px; }
  .row2 { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .opt { display: flex; align-items: center; gap: 9px; font-size: 13px; color: var(--text-2); cursor: pointer; }
  .test-row { display: flex; align-items: center; gap: 14px; min-height: 36px; }
  .result { display: flex; align-items: center; gap: 8px; font-size: 13px; min-width: 0; }
  .result.ok { color: #6ee7b7; }
  .result.err { color: #fca5a5; }
  :global(html.mobile) .grid { grid-template-columns: 1fr 1fr; gap: 12px; }
  :global(html.mobile) .span3 { grid-column: span 2; }
</style>
