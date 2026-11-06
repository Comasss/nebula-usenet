<script>
  import { t, tn } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { pickFolder } from '../lib/platform.js';
  import { fly, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Icon from './Icon.svelte';
  import { ui, queueAdd, toast, d } from '../lib/state.svelte.js';
  import * as f from '../lib/format.js';

  // Estado del análisis de cada enlace: { status: 'loading' | 'ok' | 'error', preview, error }
  let info = $state({});

  const first = $derived(ui.clipboard[0]);
  const more = $derived(ui.clipboard.length - 1);
  const cur = $derived(first ? info[first] : null);

  $effect(() => {
    const url = first;
    if (!url || info[url] || !ui.settings?.clipboardInspect) return;
    info[url] = { status: 'loading' };
    invoke('inspect_nzb_url', { url })
      .then((preview) => (info[url] = { status: 'ok', preview }))
      .catch((e) => (info[url] = { status: 'error', error: String(e) }));
  });

  function host(u) {
    try {
      return new URL(u).host;
    } catch {
      return u.slice(0, 40);
    }
  }

  function fallbackName(u) {
    try {
      const url = new URL(u);
      const last = decodeURIComponent(url.pathname).split('/').filter(Boolean).pop() ?? '';
      return last.length > 3 && last.includes('.') ? last : null;
    } catch {
      return null;
    }
  }

  function take() {
    const url = ui.clipboard.shift();
    const st = info[url];
    delete info[url];
    return { url, st };
  }

  function addOne() {
    const { url, st } = take();
    queueAdd([{ kind: 'url', value: url, preview: st?.status === 'ok' ? st.preview : null }]);
  }

  function addAll() {
    while (ui.clipboard.length) addOne();
  }

  function ignore() {
    const { st } = take();
    if (st?.status === 'ok') invoke('discard_pending', { token: st.preview.token });
  }

  const nntp = $derived(ui.settings?.featureNntp !== false);

  // Modo solo indexadores: el NZB se guarda en el PC en lugar de descargarse.
  async function saveOne() {
    let dir = ui.settings.nzbSaveDir;
    if (ui.settings.nzbAskSaveDir || !dir) {
      dir = await pickFolder({ defaultPath: dir || undefined, title: t('common.whereSaveNzb') });
      if (!dir) return;
    }
    const { url, st } = take();
    try {
      const path =
        st?.status === 'ok'
          ? await invoke('save_pending_nzb', { token: st.preview.token, dir, name: st.preview.name })
          : await invoke('save_nzb_url', { url, dir, name: '' });
      toast('success', t('common.nzbSaved', { name: path.split(/[\\/]/).pop() }), 3500);
    } catch (e) {
      toast('error', String(e));
    }
  }

  function retry() {
    delete info[first];
  }
</script>

{#if first}
  <div class="clip card" class:bad={cur?.status === 'error'} in:fly={{ y: 30, duration: d(380), easing: cubicOut }} out:fly={{ y: 30, duration: d(220) }}>
    <div class="c-icon">
      {#if cur?.status === 'loading'}
        <Icon name="loader" size={20} class="spin" />
      {:else if cur?.status === 'error'}
        <Icon name="alert-triangle" size={20} />
      {:else}
        <Icon name="clipboard" size={20} />
      {/if}
    </div>
    <div class="c-text">
      {#if cur?.status === 'loading'}
        <div class="c-title">{t('clipboard.analyzing')}</div>
        <div class="c-sub truncate" title={first}>{host(first)}</div>
      {:else if cur?.status === 'ok'}
        <div class="c-kicker">{t('clipboard.detected')}</div>
        <div class="c-title truncate" title={cur.preview.name} in:fade={{ duration: d(200) }}>{cur.preview.name}</div>
        <div class="c-sub truncate">
          {f.bytes(cur.preview.dataBytes)} · {tn('common.files', cur.preview.fileCount)}{#if cur.preview.hasArchives}{' · '}{t('clipboard.compressed')}{/if} · {host(first)}
        </div>
      {:else if cur?.status === 'error'}
        <div class="c-kicker">{t('clipboard.notNzb')}</div>
        <div class="c-sub err" title={cur.error}>{cur.error}</div>
        <div class="c-sub truncate muted" title={first}>{host(first)}</div>
      {:else}
        <div class="c-title">{t('clipboard.linkDetected')}</div>
        <div class="c-sub truncate" title={first}>{fallbackName(first) ?? host(first)}</div>
      {/if}
      {#if more > 0}<div class="c-more">{tn('clipboard.more', more)}</div>{/if}
    </div>
    <div class="c-actions">
      <button class="btn ghost sm" onclick={ignore}>{t('clipboard.ignore')}</button>
      {#if cur?.status === 'error'}
        <button class="btn sm" onclick={retry}><Icon name="refresh" size={14} /> {t('common.retry')}</button>
      {:else}
        {#if nntp}
          {#if more > 0}<button class="btn sm" onclick={addAll}>{t('clipboard.addAll')}</button>{/if}
          <button class="btn primary sm" onclick={addOne} disabled={cur?.status === 'loading'}><Icon name="plus" size={15} /> {t('common.add')}</button>
        {:else}
          <button class="btn primary sm" onclick={saveOne} disabled={cur?.status === 'loading'}><Icon name="file" size={15} /> {t('common.saveNzb')}</button>
        {/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  .clip {
    position: fixed;
    left: 50%;
    bottom: 22px;
    translate: -50% 0;
    z-index: 90;
    display: flex;
    align-items: center;
    gap: 14px;
    width: min(720px, calc(100vw - 300px));
    padding: 14px 14px 14px 16px;
    border-color: rgba(var(--accent-rgb), 0.35);
    background: linear-gradient(90deg, rgba(var(--accent-rgb), 0.1), transparent 60%), rgba(20, 23, 32, 0.97);
    box-shadow: var(--shadow), 0 0 40px -12px rgba(var(--accent-rgb), 0.55);
    backdrop-filter: blur(12px);
  }
  .clip.bad { border-color: rgba(251, 191, 36, 0.35); background: linear-gradient(90deg, rgba(251, 191, 36, 0.08), transparent 60%), rgba(20, 23, 32, 0.97); box-shadow: var(--shadow); }
  .c-icon {
    width: 42px; height: 42px;
    border-radius: 12px;
    display: grid; place-items: center;
    background: var(--grad-diag);
    color: #fff;
    flex-shrink: 0;
  }
  .bad .c-icon { background: rgba(251, 191, 36, 0.16); color: var(--warn); }
  .c-text { flex: 1; min-width: 0; }
  .c-kicker { font-size: 11.5px; color: var(--text-2); font-weight: 600; }
  .c-title { font-weight: 650; }
  .c-sub { font-size: 12.5px; color: var(--text-2); margin-top: 2px; }
  .c-sub.err { color: #fde68a; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .c-more { font-size: 12px; color: var(--muted); }
  .c-actions { display: flex; gap: 8px; flex-shrink: 0; }

  @media (max-width: 720px) {
    .clip { width: calc(100vw - 20px); bottom: 74px; flex-wrap: wrap; gap: 10px 12px; padding: 12px; }
    .c-actions { width: 100%; display: flex; justify-content: flex-end; gap: 8px; }
  }
</style>
