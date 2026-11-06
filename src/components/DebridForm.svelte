<script>
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { fly } from 'svelte/transition';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import { d } from '../lib/state.svelte.js';
  import { DEBRID_KINDS, kindInfo } from '../lib/debrid.js';

  let { provider = $bindable() } = $props();
  let showKey = $state(false);
  let testing = $state(false);
  let result = $state(null);

  const info = $derived(kindInfo(provider.kind));

  function pickKind(id) {
    if (!provider.name.trim() || DEBRID_KINDS.some((k) => k.name === provider.name)) provider.name = '';
    provider.kind = id;
    result = null;
  }

  async function test() {
    if (!provider.apiKey.trim()) {
      result = { ok: false, message: t('debrid.pasteKey') };
      return;
    }
    testing = true;
    result = null;
    try {
      result = await invoke('test_debrid', { provider: $state.snapshot(provider) });
    } catch (e) {
      result = { ok: false, message: String(e) };
    } finally {
      testing = false;
    }
  }
</script>

<div class="dbf">
  <div class="field">
    <span class="label">{t('debrid.service')}</span>
    <div class="kinds">
      {#each DEBRID_KINDS as k}
        <button type="button" class="kind" class:sel={provider.kind === k.id} onclick={() => pickKind(k.id)}>
          <Icon name="cloud" size={15} /> {k.name}
        </button>
      {/each}
    </div>
  </div>

  <div class="grid">
    <div class="field span2">
      <label for="db-key">{t('common.apiKey')}</label>
      <div class="pw">
        <input id="db-key" class="input mono" type={showKey ? 'text' : 'password'} bind:value={provider.apiKey} autocomplete="off" spellcheck="false" placeholder={t('debrid.keyPlaceholder')} />
        <button type="button" class="icon-btn eye" onclick={() => (showKey = !showKey)} aria-label={t('common.showApiKey')}><Icon name={showKey ? 'eye-off' : 'eye'} size={16} /></button>
      </div>
      <div class="hint">
        {info.keyHelp}
        <button type="button" class="link" onclick={() => invoke('open_url', { url: info.keyUrl })}>{t('common.open')} <Icon name="external" size={11} /></button>
      </div>
    </div>
    <div class="field">
      <label for="db-name">{t('common.name')} <span class="muted">({t('common.optional')})</span></label>
      <input id="db-name" class="input" placeholder={info.name} bind:value={provider.name} />
    </div>
  </div>

  <label class="opt"><Switch checked={provider.enabled} onchange={(v) => (provider.enabled = v)} /> {t('common.enabled')}</label>

  <div class="test-row">
    <button type="button" class="btn" onclick={test} disabled={testing}>
      {#if testing}<Icon name="loader" size={15} class="spin" /> {t('common.testing')}{:else}<Icon name="plug" size={15} /> {t('debrid.testKey')}{/if}
    </button>
    {#if result}
      <div class="result {result.ok ? 'ok' : 'err'}" in:fly={{ x: -10, duration: d(250) }}>
        <Icon name={result.ok ? 'check-circle' : 'x-circle'} size={16} />
        <span class="selectable">{result.message}{#if result.ok && result.latencyMs}{' · '}{result.latencyMs} ms{/if}</span>
      </div>
    {/if}
  </div>
  {#if result?.warning}
    <div class="alert warn" in:fly={{ y: -6, duration: d(200) }}><Icon name="alert-triangle" size={15} /><span>{result.warning}</span></div>
  {/if}
</div>

<style>
  .dbf { display: flex; flex-direction: column; gap: 16px; }
  .kinds { display: flex; flex-wrap: wrap; gap: 6px; }
  .kind {
    display: inline-flex; align-items: center; gap: 7px;
    height: 34px; padding: 0 14px; border-radius: 9px;
    border: 1px solid var(--border-2); background: var(--panel-2);
    color: var(--text-2); font-size: 13px; font-weight: 600; cursor: pointer;
    transition: all 0.15s;
  }
  .kind:hover { color: var(--text); background: var(--panel-3); }
  .kind.sel { border-color: rgba(var(--accent-rgb), 0.6); background: rgba(var(--accent-rgb), 0.16); color: var(--text); }
  .grid { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 14px; }
  .span2 { grid-column: span 2; }
  .pw { position: relative; }
  .pw .input { padding-right: 40px; }
  .eye { position: absolute; right: 3px; top: 3px; }
  .hint { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .link { display: inline-flex; align-items: center; gap: 3px; border: none; background: none; color: var(--accent-2); cursor: pointer; font-weight: 600; padding: 0; font-size: inherit; }
  .opt { display: flex; align-items: center; gap: 9px; font-size: 13px; color: var(--text-2); cursor: pointer; align-self: flex-start; }
  .test-row { display: flex; align-items: center; gap: 14px; min-height: 36px; }
  .result { display: flex; align-items: center; gap: 8px; font-size: 13px; min-width: 0; }
  .result.ok { color: #6ee7b7; }
  .result.err { color: #fca5a5; }
  :global(html.mobile) .grid { grid-template-columns: 1fr 1fr; gap: 12px; }
</style>
