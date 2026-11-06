<script>
  import { t, tn, th } from '../lib/i18n.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { pickFolder } from '../lib/platform.js';
  import { slide, fade } from 'svelte/transition';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import { ui, toast, d } from '../lib/state.svelte.js';
  import * as f from '../lib/format.js';
  import { usableDebrid, debridLabel, hasNntp, kindInfo, nntpServers, serverLabel, viaLabel } from '../lib/debrid.js';
  import { haptic, flyTo } from '../lib/feedback.js';
  import Select from './Select.svelte';
  import { categoryOptions, categoryDefaults, priorityOptions, DEFAULT_CAT } from '../lib/categories.js';

  const current = $derived(ui.addItems[0]);
  let phase = $state('idle'); // loading | preview | error
  let preview = $state(null);
  let error = $state('');
  let adding = $state(false);
  let showPw = $state(false);
  let showFiles = $state(false);
  let free = $state(null);
  let form = $state({ name: '', dest: '', password: '', paused: false, top: false, via: 'nntp', options: {}, category: DEFAULT_CAT, priority: 0 });
  let cached = $state([]);
  let loadedKey = null;

  $effect(() => {
    const it = current;
    if (!it) return;
    const key = `${it.kind}:${it.value}`;
    if (key === loadedKey) return;
    loadedKey = key;
    load(it);
  });

  async function load(it) {
    phase = 'loading';
    error = '';
    preview = null;
    showFiles = false;
    cached = [];
    try {
      // Si el aviso del portapapeles ya analizó el enlace, se reutiliza (no se vuelve a pedir al indexador).
      const p = it.preview
        ? $state.snapshot(it.preview)
        : it.kind === 'file'
          ? await invoke('inspect_nzb_file', { path: it.value })
          : await invoke('inspect_nzb_url', { url: it.value });
      const s = ui.settings;
      preview = p;
      // La categoría (elegida o deducida de las categorías Newznab del resultado) pone la carpeta,
      // el método, la prioridad y el post-procesado.
      const cat = categoryDefaults(s, it.category);
      form = {
        name: p.name,
        dest: cat.dest || p.defaultDir,
        password: p.password ?? '',
        paused: s.addPaused,
        top: false,
        via: it.via ?? cat.via,
        options: cat.options,
        category: cat.category,
        priority: cat.priority,
      };
      phase = 'preview';
      checkCache(p.token);
      if (!s.askDestination && s.featureNntp !== false) {
        await confirm();
        return;
      }
      updateFree();
    } catch (e) {
      error = String(e);
      phase = 'error';
    }
  }

  // ¿Algún servicio debrid lo tiene ya en caché? (descarga instantánea)
  async function checkCache(token) {
    if (!usableDebrid(ui.settings).some((p) => kindInfo(p.kind).cache)) return;
    try {
      const ids = await invoke('debrid_cached', { token });
      if (preview?.token === token) cached = ids;
    } catch {}
  }

  function setCategory(name) {
    const cat = categoryDefaults(ui.settings, name);
    form.category = cat.category;
    form.dest = cat.dest;
    form.via = current?.via ?? cat.via;
    form.options = cat.options;
    form.priority = cat.priority;
    updateFree();
  }

  const catOptions = $derived(categoryOptions(ui.settings));

  async function updateFree() {
    try {
      free = await invoke('disk_free', { path: form.dest });
    } catch {
      free = null;
    }
  }

  async function browse() {
    const dir = await pickFolder({ defaultPath: form.dest || undefined, title: t('add.whereSaveDownload') });
    if (dir) {
      form.dest = dir;
      updateFree();
    }
  }

  async function confirm() {
    if (!preview || adding) return;
    adding = true;
    try {
      await invoke('add_job', {
        req: {
          token: preview.token,
          name: form.name.trim(),
          destDir: form.dest.trim(),
          password: form.password.trim() || null,
          paused: form.paused,
          top: form.top,
          via: form.via,
          options: $state.snapshot(form.options),
          category: form.category,
          priority: form.priority,
        },
      });
      const viaName = form.via !== 'nntp' ? viaLabel(ui.settings, form.via) : '';
      haptic('success');
      flyTo(document.querySelector('.add-confirm'), 'downloads');
      toast('success', viaName ? t('add.queuedVia', { provider: viaName, name: form.name }) : t('add.queued', { name: form.name }), 3500);
      if (ui.view !== 'downloads') ui.view = 'downloads';
      next();
    } catch (e) {
      if (String(e).startsWith('⌛') && current) {
        current.preview = null;
        retry();
      } else {
        toast('error', String(e));
      }
    } finally {
      adding = false;
    }
  }

  const nntp = $derived(ui.settings?.featureNntp !== false);
  const debridOptions = $derived(usableDebrid(ui.settings));
  const servers = $derived(nntpServers(ui.settings));
  const noServers = $derived(!hasNntp(ui.settings));

  async function saveNzb() {
    if (!preview || adding) return;
    let dir = ui.settings.nzbSaveDir;
    if (ui.settings.nzbAskSaveDir || !dir) {
      dir = await pickFolder({ defaultPath: dir || undefined, title: t('common.whereSaveNzb') });
      if (!dir) return;
    }
    adding = true;
    try {
      const path = await invoke('save_pending_nzb', { token: preview.token, dir, name: form.name.trim() });
      toast('success', t('common.nzbSaved', { name: path.split(/[\\/]/).pop() }), 3500);
      preview = null;
      next();
    } catch (e) {
      toast('error', String(e));
    } finally {
      adding = false;
    }
  }

  function cancel() {
    if (preview) invoke('discard_pending', { token: preview.token });
    next();
  }

  function retry() {
    loadedKey = null;
    if (current) load(current);
  }

  function next() {
    ui.addItems.shift();
    loadedKey = null;
    preview = null;
    phase = 'idle';
  }

  function sourceLabel(it) {
    if (!it) return '';
    if (it.kind === 'file') return it.value.split(/[\\/]/).pop();
    try {
      return new URL(it.value).host;
    } catch {
      return it.value;
    }
  }

  const needed = $derived(preview ? preview.dataBytes : 0);
  const lowSpace = $derived(free !== null && needed > 0 && free < needed * 1.1);
  const remainingItems = $derived(ui.addItems.length - 1);
</script>

{#if current}
  <Modal
    title={phase === 'error' ? t('add.titleError') : phase === 'loading' ? t('add.titleLoading') : t('add.title')}
    subtitle={remainingItems > 0 ? `${sourceLabel(current)} · ${tn('add.pending', remainingItems)}` : sourceLabel(current)}
    icon={phase === 'error' ? 'x-circle' : current.kind === 'file' ? 'file' : 'link'}
    iconKind={phase === 'error' ? 'err' : 'accent'}
    width={640}
    onclose={cancel}>
    {#if phase === 'loading'}
      <div class="loading" in:fade={{ duration: d(200) }}>
        <div class="loader"><Icon name="loader" size={30} class="spin" /></div>
        <div>{current.kind === 'file' ? t('add.readingFile') : t('add.fetchingLink')}</div>
        {#if current.kind !== 'file'}<div class="muted small truncate">{current.value}</div>{/if}
      </div>
    {:else if phase === 'error'}
      <div class="alert err" in:fade={{ duration: d(200) }}>
        <Icon name="alert-triangle" size={17} />
        <span class="selectable">{error}</span>
      </div>
      {#if current.kind !== 'file'}
        <p class="muted small url selectable">{current.value}</p>
      {/if}
    {:else if phase === 'preview' && preview}
      <div class="pv" in:fade={{ duration: d(200) }}>
        <div class="summary">
          <div class="sum"><span class="k">{t('common.size')}</span><span class="v">{f.bytes(preview.dataBytes)}</span></div>
          <div class="sum"><span class="k">{t('add.files')}</span><span class="v">{preview.fileCount}</span></div>
          <div class="sum"><span class="k">{t('add.recovery')}</span><span class="v">{preview.par2Bytes ? f.bytes(preview.par2Bytes) : t('add.noPar2')}</span></div>
          <div class="sum"><span class="k">{t('add.posted')}</span><span class="v">{preview.oldestPost ? f.ago(preview.oldestPost) : '—'}</span></div>
        </div>

        {#if preview.retentionWarning}
          <div class="alert warn"><Icon name="clock" size={16} /><span>{preview.retentionWarning}</span></div>
        {/if}

        <div class="field">
          <label for="add-name">{t('common.name')}</label>
          <input id="add-name" class="input" bind:value={form.name} />
        </div>

        {#if !nntp}
          <div class="alert info">
            <Icon name="info" size={16} />
            <span>{t('add.nntpDisabled', { dir: ui.settings.nzbAskSaveDir ? t('add.askFolder') : ui.settings.nzbSaveDir })}</span>
          </div>
        {:else}
        {#if catOptions.length > 1}
          <div class="cat-row">
            <div class="field">
              <span class="label">{t('cats.category')}</span>
              <Select value={form.category} options={catOptions} label={t('cats.category')} onchange={setCategory} />
            </div>
            <div class="field">
              <span class="label">{t('cats.priority')}</span>
              <Select bind:value={form.priority} options={priorityOptions()} label={t('cats.priority')} />
            </div>
          </div>
        {/if}

        {#if debridOptions.length || servers.length > 1}
          <div class="field">
            <span class="label">{t('add.downloadWith')}</span>
            <div class="via">
              <button type="button" class="via-btn" class:sel={form.via === 'nntp'} onclick={() => (form.via = 'nntp')}>
                <Icon name="server" size={15} />
                <span class="via-text"><strong>Usenet (NNTP)</strong><small>{noServers ? t('add.noServersConfigured') : servers.length > 1 ? t('via.allServers') : t('add.yourServers')}</small></span>
              </button>
              {#if servers.length > 1}
                {#each servers as srv (srv.id)}
                  <button type="button" class="via-btn" class:sel={form.via === `nntp:${srv.id}`} onclick={() => (form.via = `nntp:${srv.id}`)}>
                    <Icon name="server" size={15} />
                    <span class="via-text"><strong>{serverLabel(srv)}</strong><small>{srv.priority === servers[0].priority ? t('via.primary') : t('via.backup', { n: srv.priority })}</small></span>
                  </button>
                {/each}
              {/if}
              {#each debridOptions as p (p.id)}
                <button type="button" class="via-btn" class:sel={form.via === p.id} onclick={() => (form.via = p.id)}>
                  <Icon name="cloud" size={15} />
                  <span class="via-text">
                    <strong>{debridLabel(p)}</strong>
                    <small>{cached.includes(p.id) ? t('add.cached') : t('add.remoteDownload')}</small>
                  </span>
                  {#if cached.includes(p.id)}<span class="bolt" title={t('add.cachedTitle')}><Icon name="zap" size={13} /></span>{/if}
                </button>
              {/each}
            </div>
          </div>
        {/if}

        <div class="field">
          <label for="add-dest">{t('add.whereSave')}</label>
          <div class="dest">
            <div class="dest-input">
              <Icon name="folder" size={16} />
              <input id="add-dest" class="input" bind:value={form.dest} onchange={updateFree} />
            </div>
            <button class="btn" onclick={browse}>{t('common.browse')}…</button>
          </div>
          <div class="hint dest-hint" class:bad={lowSpace}>
            {#if free !== null}
              <Icon name="hard-drive" size={12} />
              {t('add.free', { size: f.bytes(free) })}{#if lowSpace} — {t('add.lowSpace')}{/if}
            {/if}
            {#if form.options.createFolder}
              <span class="path truncate">→ {form.dest.replace(/[\\/]+$/, '')}\{form.name}</span>
            {/if}
          </div>
        </div>

        <div class="field">
          <label for="add-pw">{t('add.archivePassword')} <span class="muted">({t('common.optional')})</span></label>
          <div class="pw">
            <Icon name="key" size={16} />
            <input
              id="add-pw"
              class="input"
              type={showPw ? 'text' : 'password'}
              bind:value={form.password}
              placeholder={preview.password ? '' : t('add.passwordPlaceholder')} />
            <button class="icon-btn eye" onclick={() => (showPw = !showPw)} aria-label={t('common.showPassword')}><Icon name={showPw ? 'eye-off' : 'eye'} size={16} /></button>
          </div>
        </div>

        <div class="opts">
          <label class="opt"><Switch checked={form.options.createFolder} onchange={(v) => (form.options.createFolder = v)} /> {t('add.createFolder')}</label>
          <label class="opt" title={t('add.par2Title')}><Switch checked={form.options.par2} onchange={(v) => (form.options.par2 = v)} /> {t('add.par2')}</label>
          <label class="opt"><Switch checked={form.options.extract} onchange={(v) => (form.options.extract = v)} /> {t('add.extract')}</label>
          <label class="opt"><Switch checked={form.options.deleteArchives} onchange={(v) => (form.options.deleteArchives = v)} disabled={!form.options.extract} /> {t('add.deleteArchives')}</label>
          <label class="opt"><Switch checked={form.top} onchange={(v) => (form.top = v)} /> {t('add.top')}</label>
          <label class="opt"><Switch checked={form.paused} onchange={(v) => (form.paused = v)} /> {t('add.paused')}</label>
        </div>

        {/if}

        <button class="files-toggle" onclick={() => (showFiles = !showFiles)}>
          <Icon name="chevron-right" size={14} class={showFiles ? 'rot' : ''} />
          {t('add.showContents')} ({tn('common.files', preview.fileCount)}{#if preview.groups.length}{' · '}{preview.groups[0]}{/if})
        </button>
        {#if showFiles}
          <div class="files" transition:slide={{ duration: d(220) }}>
            {#each preview.files as file}
              <div class="file"><Icon name={file.par2 ? 'shield' : 'file'} size={13} /><span class="truncate">{file.name}</span><span class="muted">{f.bytes(file.bytes)}</span></div>
            {/each}
            {#if preview.fileCount > preview.files.length}<div class="muted small">{tn('add.moreFiles', preview.fileCount - preview.files.length)}</div>{/if}
          </div>
        {/if}
      </div>
    {/if}

    {#snippet footer()}
      {#if phase === 'error'}
        <button class="btn ghost" onclick={cancel}>{t('add.discard')}</button>
        <button class="btn primary" onclick={retry}><Icon name="refresh" size={15} /> {t('common.retry')}</button>
      {:else}
        <button class="btn ghost" onclick={cancel}>{t('common.cancel')}</button>
        {#if !nntp}
          <button class="btn primary" onclick={saveNzb} disabled={phase !== 'preview' || adding}><Icon name="file" size={16} /> {t('common.saveNzb')}</button>
        {:else}
        <button class="btn primary add-confirm" onclick={confirm} disabled={phase !== 'preview' || adding || !form.dest.trim()}>
          {#if adding}<Icon name="loader" size={15} class="spin" />{:else}<Icon name="download" size={16} />{/if}
          {t('common.download')}
        </button>
        {/if}
      {/if}
    {/snippet}
  </Modal>
{/if}

<style>
  .loading { display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 26px 0 16px; color: var(--text-2); }
  .loader { width: 58px; height: 58px; border-radius: 18px; display: grid; place-items: center; background: rgba(var(--accent-rgb), 0.14); color: var(--accent-2); }
  .small { font-size: 12px; max-width: 100%; }
  .url { word-break: break-all; margin: 10px 0 0; }
  .pv { display: flex; flex-direction: column; gap: 16px; }
  .summary { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; }
  .sum { padding: 10px 12px; border-radius: 11px; background: var(--panel-2); border: 1px solid var(--border); display: flex; flex-direction: column; }
  .k { font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); font-weight: 650; }
  .v { font-family: var(--font-display); font-weight: 650; font-size: 15px; margin-top: 2px; }
  .dest { display: flex; gap: 8px; }
  .dest-input, .pw { position: relative; flex: 1; }
  .dest-input :global(svg:first-child), .pw > :global(svg:first-child) { position: absolute; left: 12px; top: 11px; color: var(--muted); pointer-events: none; }
  .dest-input .input, .pw .input { padding-left: 38px; }
  .pw .input { padding-right: 40px; }
  .eye { position: absolute; right: 3px; top: 3px; }
  .dest-hint { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .dest-hint.bad { color: var(--warn); }
  .path { margin-left: auto; max-width: 60%; opacity: 0.8; }
  .via { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 8px; }
  .via-btn {
    position: relative;
    display: flex; align-items: center; gap: 10px;
    padding: 9px 12px; border-radius: 11px;
    border: 1px solid var(--border-2); background: var(--panel-2);
    color: var(--text-2); cursor: pointer; text-align: left;
    transition: border-color 0.15s, background 0.15s, color 0.15s;
  }
  .via-btn:hover { color: var(--text); background: var(--panel-3); }
  .via-btn.sel { border-color: rgba(var(--accent-rgb), 0.65); background: rgba(var(--accent-rgb), 0.14); color: var(--text); }
  .via-btn.sel > :global(svg) { color: var(--accent-2); }
  .via-text { display: flex; flex-direction: column; min-width: 0; }
  .via-text strong { font-size: 13px; font-weight: 620; }
  .via-text small { font-size: 11.5px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .bolt { position: absolute; top: 6px; right: 8px; color: #fcd34d; }
  .cat-row { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .opts { display: grid; grid-template-columns: 1fr 1fr; gap: 10px 18px; padding: 14px; border-radius: 12px; background: var(--panel-2); border: 1px solid var(--border); }
  .opt { display: flex; align-items: center; gap: 10px; font-size: 13px; color: var(--text-2); cursor: pointer; }
  .files-toggle { align-self: flex-start; display: flex; align-items: center; gap: 6px; border: none; background: none; color: var(--text-2); cursor: pointer; padding: 0; font-size: 13px; }
  .files-toggle:hover { color: var(--text); }
  .files-toggle :global(svg) { transition: transform 0.25s var(--ease); }
  .files-toggle :global(.rot) { transform: rotate(90deg); }
  .files { max-height: 200px; overflow-y: auto; border: 1px solid var(--border); border-radius: 10px; padding: 6px; }
  .file { display: grid; grid-template-columns: 14px 1fr auto; gap: 8px; align-items: center; padding: 4px 6px; font-size: 12.5px; color: var(--text-2); }

  @media (max-width: 720px) {
    .summary { grid-template-columns: 1fr 1fr; gap: 8px; }
    .dest { flex-direction: column; }
    .opts { grid-template-columns: 1fr; }
    .cat-row { grid-template-columns: 1fr; }
    .path { max-width: 50%; }
  }
</style>
