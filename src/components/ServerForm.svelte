<script>
  import Select from './Select.svelte';
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { fly } from 'svelte/transition';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import { d } from '../lib/state.svelte.js';

  let { server = $bindable(), compact = false } = $props();
  let showPw = $state(false);
  let testing = $state(false);
  let result = $state(null);

  function toggleSsl(v) {
    server.ssl = v;
    if (v && server.port === 119) server.port = 563;
    if (!v && server.port === 563) server.port = 119;
  }

  export async function test() {
    if (!server.host.trim()) {
      result = { ok: false, message: t('server.hostRequired') };
      return false;
    }
    testing = true;
    result = null;
    try {
      result = await invoke('test_server', { server: $state.snapshot(server) });
    } catch (e) {
      result = { ok: false, message: String(e) };
    } finally {
      testing = false;
    }
    return result.ok;
  }
</script>

<div class="sf" class:compact>
  <div class="grid">
    <div class="field span2">
      <label for="sf-host">{t('server.host')}</label>
      <input id="sf-host" class="input" placeholder={t('server.hostPlaceholder')} bind:value={server.host} spellcheck="false" />
    </div>
    <div class="field">
      <label for="sf-port">{t('server.port')}</label>
      <input id="sf-port" class="input" type="number" min="1" max="65535" bind:value={server.port} />
    </div>
    <div class="field">
      <label for="sf-user">{t('server.username')}</label>
      <input id="sf-user" class="input" bind:value={server.username} autocomplete="off" spellcheck="false" />
    </div>
    <div class="field span2">
      <label for="sf-pass">{t('common.password')}</label>
      <div class="pw">
        <input id="sf-pass" class="input" type={showPw ? 'text' : 'password'} bind:value={server.password} autocomplete="new-password" />
        <button class="icon-btn eye" onclick={() => (showPw = !showPw)} aria-label={t('common.showPassword')}><Icon name={showPw ? 'eye-off' : 'eye'} size={16} /></button>
      </div>
    </div>
    <div class="field span2">
      <label for="sf-conns">{t('server.connections')} <span class="val">{server.connections}</span></label>
      <input id="sf-conns" class="range" type="range" min="1" max="100" bind:value={server.connections} style="--p:{(server.connections - 1) / 0.99}%" />
      <span class="hint">{t('server.connectionsHint')}</span>
    </div>
    <div class="field">
      <label for="sf-name">{t('common.name')} ({t('common.optional')})</label>
      <input id="sf-name" class="input" placeholder={server.host || t('server.namePlaceholder')} bind:value={server.name} />
    </div>
    {#if !compact}
      <div class="field">
        <label for="sf-prio">{t('common.priority')}</label>
        <Select id="sf-prio" label={t('common.priority')} bind:value={server.priority}
          options={[{ value: 0, label: t('server.primary') }, ...[1, 2, 3, 4, 5].map((p) => ({ value: p, label: t('server.backupN', { n: p }) }))]} />
      </div>
      <div class="field">
        <label for="sf-ret">{t('server.retention')}</label>
        <input id="sf-ret" class="input" type="number" min="0" bind:value={server.retentionDays} placeholder={t('server.retentionPlaceholder')} />
      </div>
    {/if}
  </div>

  <div class="toggles">
    <label class="opt"><Switch checked={server.ssl} onchange={toggleSsl} /> <Icon name="lock" size={14} /> {t('server.ssl')}</label>
    {#if server.ssl}
      <label class="opt"><Switch checked={server.verifyCert} onchange={(v) => (server.verifyCert = v)} /> {t('server.verifyCert')}</label>
    {/if}
    {#if !compact}
      <label class="opt"><Switch checked={server.enabled} onchange={(v) => (server.enabled = v)} /> {t('server.enabled')}</label>
    {/if}
  </div>

  <div class="test-row">
    <button class="btn" onclick={test} disabled={testing}>
      {#if testing}<Icon name="loader" size={15} class="spin" /> {t('common.testing')}{:else}<Icon name="plug" size={15} /> {t('server.test')}{/if}
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
  .sf { display: flex; flex-direction: column; gap: 16px; }
  .grid { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 14px; }
  .span2 { grid-column: span 2; }
  .pw { position: relative; }
  .pw .input { padding-right: 40px; }
  .eye { position: absolute; right: 3px; top: 3px; }
  .val { float: right; color: var(--text); font-variant-numeric: tabular-nums; }
  .range {
    -webkit-appearance: none; appearance: none;
    width: 100%; height: 6px; margin: 10px 0 4px;
    border-radius: 99px; outline: none;
    background: linear-gradient(90deg, var(--accent), var(--accent-2)) 0 / var(--p) 100% no-repeat, var(--panel-4);
  }
  .range::-webkit-slider-thumb {
    -webkit-appearance: none; appearance: none;
    width: 18px; height: 18px; border-radius: 50%;
    background: #fff; cursor: pointer;
    box-shadow: 0 0 0 4px rgba(var(--accent-rgb), 0.3), 0 2px 6px rgba(0, 0, 0, 0.4);
    transition: box-shadow 0.2s;
  }
  .range::-webkit-slider-thumb:hover { box-shadow: 0 0 0 6px rgba(var(--accent-rgb), 0.3), 0 2px 6px rgba(0, 0, 0, 0.4); }
  .toggles { display: flex; flex-wrap: wrap; gap: 12px 24px; }
  .opt { display: flex; align-items: center; gap: 9px; font-size: 13px; color: var(--text-2); cursor: pointer; }
  .test-row { display: flex; align-items: center; gap: 14px; min-height: 36px; }
  .result { display: flex; align-items: center; gap: 8px; font-size: 13px; min-width: 0; }
  .result.ok { color: #6ee7b7; }
  .result.err { color: #fca5a5; }
  :global(html.mobile) .grid { grid-template-columns: 1fr 1fr; gap: 12px; }
</style>
