<script>
  // Pasar los ajustes a otro dispositivo: QR para enviar o recibir, pegar el código y confirmar.
  import { t } from '../lib/i18n.svelte.js';
  import { isAndroid } from '../lib/platform.js';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import { tr, closeTransfer, showReceive, handleCode, confirmImport, confirmPush, copyCode, cancelScan } from '../lib/transfer.svelte.js';

  const TITLES = {
    send: ['transfer.sendTitle', 'qr'],
    receive: ['transfer.receiveTitle', 'wifi'],
    paste: ['transfer.pasteTitle', 'clipboard'],
    import: ['transfer.importTitle', 'download'],
    push: ['transfer.pushTitle', 'wifi'],
  };
  const head = $derived(TITLES[tr.mode] ?? ['', 'info']);
  const p = $derived(tr.preview);
  const from = $derived(p?.from === 'android' ? t('transfer.fromAndroid') : p?.from === 'windows' ? t('transfer.fromWindows') : p?.from ? t('transfer.fromDesktop') : '');
</script>

{#if tr.mode}
  <div class="layer">
    <Modal title={t(head[0])} subtitle={tr.mode === 'import' ? from : ''} icon={head[1]} width={440} onclose={closeTransfer} dismissable={!tr.busy}>
      {#if tr.mode === 'send' || tr.mode === 'receive'}
        {#if tr.loading}
          <div class="qr-box"><div class="spinner"></div></div>
        {:else if tr.qr?.svg && !tr.error}
          <div class="qr-box"><div class="qr">{@html tr.qr.svg}</div></div>
          <p class="hint">{tr.mode === 'send' ? t('transfer.sendHint') : t('transfer.receiveHint')}</p>
          {#if tr.mode === 'send'}
            <div class="alert warn"><Icon name="key" size={15} /> <span>{t('transfer.secret')}</span></div>
          {:else}
            <p class="waiting"><span class="dot"></span> {t('transfer.waiting')}</p>
            {#if !isAndroid}<p class="hint small">{t('transfer.firewall')}</p>{/if}
          {/if}
        {/if}
        {#if tr.error}
          <div class="alert {tr.mode === 'send' ? 'info' : 'err'}"><Icon name="alert-triangle" size={15} /> <span>{tr.error}</span></div>
        {/if}
      {:else if tr.mode === 'paste'}
        <p class="hint left">{t('transfer.pasteDesc')}</p>
        <!-- svelte-ignore a11y_autofocus -->
        <textarea class="input code" rows="5" placeholder="NEBULA1:…" bind:value={tr.text} autofocus spellcheck="false"></textarea>
        {#if tr.error}<div class="alert err"><Icon name="alert-triangle" size={15} /> <span>{tr.error}</span></div>{/if}
      {:else if tr.mode === 'import' && p}
        <div class="counts">
          <div><Icon name="server" size={16} /> <span>{t('nav.servers')}</span> <strong>{p.servers}</strong></div>
          <div><Icon name="layers" size={16} /> <span>{t('nav.indexers')}</span> <strong>{p.indexers}</strong></div>
          <div><Icon name="cloud" size={16} /> <span>{t('nav.debrid')}</span> <strong>{p.debrid}</strong></div>
          {#if p.rss}<div><Icon name="rss" size={16} /> <span>{t('rss.rules')}</span> <strong>{p.rss}</strong></div>{/if}
          {#if p.api}<div><Icon name="plug" size={16} /> <span>{t('api.title')}</span> <Icon name="check" size={15} stroke={3} /></div>{/if}
        </div>
        <p class="hint left">{t('transfer.importNote')}</p>
        {#if tr.error}<div class="alert err"><Icon name="alert-triangle" size={15} /> <span>{tr.error}</span></div>{/if}
      {:else if tr.mode === 'push' && p}
        <p class="hint left">{t('transfer.pushText', { address: p.address })}</p>
        {#if tr.error}<div class="alert err"><Icon name="alert-triangle" size={15} /> <span>{tr.error}</span></div>{/if}
      {/if}

      {#snippet footer()}
        {#if tr.mode === 'send'}
          <button class="btn ghost" onclick={closeTransfer}>{t('common.close')}</button>
          <button class="btn primary" onclick={copyCode} disabled={!tr.qr?.code}><Icon name="copy" size={15} /> {t('transfer.copyCode')}</button>
        {:else if tr.mode === 'receive'}
          <button class="btn ghost" onclick={closeTransfer}>{t('common.close')}</button>
          {#if tr.error}<button class="btn primary" onclick={showReceive}><Icon name="refresh" size={15} /> {t('common.retry')}</button>{/if}
        {:else if tr.mode === 'paste'}
          <button class="btn ghost" onclick={closeTransfer}>{t('common.cancel')}</button>
          <button class="btn primary" onclick={() => handleCode(tr.text)} disabled={!tr.text.trim()}>{t('transfer.continue')}</button>
        {:else if tr.mode === 'import'}
          <button class="btn ghost" onclick={closeTransfer} disabled={tr.busy}>{t('common.cancel')}</button>
          <button class="btn primary" onclick={confirmImport} disabled={tr.busy}><Icon name="download" size={15} /> {t('transfer.import')}</button>
        {:else if tr.mode === 'push'}
          <button class="btn ghost" onclick={closeTransfer} disabled={tr.busy}>{t('common.cancel')}</button>
          <button class="btn primary" onclick={confirmPush} disabled={tr.busy}><Icon name="wifi" size={15} /> {tr.busy ? t('transfer.sending') : t('transfer.send')}</button>
        {/if}
      {/snippet}
    </Modal>
  </div>
{/if}

{#if tr.scanning}
  <div class="scan-layer">
    <div class="frame"></div>
    <p>{t('transfer.scanHint')}</p>
    <button class="btn" onclick={cancelScan}><Icon name="x" size={16} /> {t('common.cancel')}</button>
  </div>
{/if}

<style>
  /* Por encima del asistente de primera configuración. */
  .layer { position: relative; z-index: 250; }
  .qr-box { display: grid; place-items: center; min-height: 220px; }
  .qr { width: min(300px, 72vw); padding: 10px; border-radius: 14px; background: #fff; line-height: 0; }
  .qr :global(svg) { width: 100%; height: auto; }
  .hint { margin: 14px 0 0; text-align: center; color: var(--text-2); font-size: 13px; }
  .hint.left { text-align: left; margin: 0 0 12px; }
  .hint.small { font-size: 12px; color: var(--muted); margin-top: 6px; }
  .alert { margin-top: 12px; }
  .waiting { display: flex; align-items: center; justify-content: center; gap: 8px; margin: 10px 0 0; font-size: 13px; color: var(--accent-2); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent-2); animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.25; } }
  .code { width: 100%; font-family: var(--font-mono, monospace); font-size: 12px; word-break: break-all; user-select: text; }
  .counts { display: flex; flex-direction: column; gap: 2px; margin-bottom: 14px; padding: 6px 12px; border: 1px solid var(--border); border-radius: 12px; background: var(--panel-2); }
  .counts div { display: flex; align-items: center; gap: 10px; padding: 7px 0; color: var(--text-2); }
  .counts span { flex: 1; }
  .counts strong { color: var(--text); font-variant-numeric: tabular-nums; }

  /* Escaneando: la cámara está detrás del WebView, que se vuelve transparente salvo esta capa. */
  :global(html.scanning), :global(html.scanning body) { background: transparent !important; }
  :global(html.scanning #app > *:not(.scan-layer)) { visibility: hidden; }
  .scan-layer {
    position: fixed; inset: 0; z-index: 400;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 22px;
    padding: 24px;
    color: #fff;
  }
  .frame { width: min(70vw, 300px); aspect-ratio: 1; border-radius: 24px; border: 3px solid rgba(255, 255, 255, 0.9); box-shadow: 0 0 0 100vmax rgba(0, 0, 0, 0.45); }
  .scan-layer p { position: relative; margin: 0; text-align: center; font-size: 14px; text-shadow: 0 1px 4px rgba(0, 0, 0, 0.8); }
  .scan-layer .btn { position: relative; }
</style>
