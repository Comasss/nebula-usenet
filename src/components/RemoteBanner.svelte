<script>
  // Aviso fijo mientras el móvil controla un PC: a quién, si hay conexión y cómo volver al móvil.
  import { t } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';
  import { remote, stopControl } from '../lib/remote.svelte.js';

  const a = $derived(remote.active);
  const online = $derived(a?.status === 'online');
</script>

{#if a}
  <div class="rb" class:off={a.status === 'offline'} class:wait={a.status === 'connecting'}>
    <span class="ic"><Icon name="monitor" size={17} /></span>
    <div class="txt">
      <strong class="truncate">{t('remote.controlling', { name: a.name })}</strong>
      <small>
        {#if online}
          <span class="dot"></span> {t('remote.online')}
        {:else if a.status === 'connecting'}
          {t('remote.connecting')}
        {:else}
          {a.error || t('remote.offline', { name: a.name })} {t('remote.retrying')}
        {/if}
      </small>
    </div>
    <button class="btn sm" onclick={stopControl}><Icon name="smartphone" size={14} /> {t('remote.backToPhone')}</button>
  </div>
{/if}

<style>
  .rb {
    grid-area: 1 / 1;
    position: relative;
    z-index: 4;
    display: flex; align-items: center; gap: 12px;
    margin: 10px 12px 0;
    padding: 10px 10px 10px 12px;
    border-radius: 16px;
    border: 1px solid rgba(var(--accent-rgb), 0.35);
    background: linear-gradient(135deg, rgba(var(--accent-rgb), 0.2), rgba(var(--accent-rgb), 0.06)), var(--panel);
  }
  .rb.off { border-color: rgba(251, 191, 36, 0.45); background: linear-gradient(135deg, rgba(251, 191, 36, 0.16), transparent), var(--panel); }
  .ic { width: 34px; height: 34px; border-radius: 11px; flex-shrink: 0; display: grid; place-items: center; background: rgba(var(--accent-rgb), 0.22); color: var(--text); }
  .off .ic { background: rgba(251, 191, 36, 0.2); color: #fbbf24; }
  .txt { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .txt strong { font-size: 14px; }
  .txt small { font-size: 12px; color: var(--text-2); line-height: 1.35; }
  .dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: #34d399; margin-right: 3px; vertical-align: 1px; box-shadow: 0 0 0 3px rgba(52, 211, 153, 0.2); }
  .wait .ic { animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.5; } }
  .btn { flex-shrink: 0; }
</style>
