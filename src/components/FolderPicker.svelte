<script>
  // Selector de carpetas propio (Android no tiene uno que devuelva rutas): navega por el
  // almacenamiento con el mismo estilo que el resto de Nebula.
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { fade } from 'svelte/transition';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import { ui, ask, toast, d } from '../lib/state.svelte.js';

  const ROOT = '/storage/emulated/0';
  const req = $derived(ui.folderPicker);
  let path = $state('');
  let dirs = $state([]);
  let parent = $state(null);
  let loading = $state(false);
  let error = $state('');
  let opened = null;

  $effect(() => {
    if (req && req !== opened) {
      opened = req;
      go(req.path || `${ROOT}/Download`, true);
    }
  });

  async function go(p, fallback = false) {
    loading = true;
    error = '';
    try {
      const r = await invoke('list_dirs', { path: p });
      path = r.path;
      parent = r.path === ROOT || r.path === '/' ? null : r.parent;
      dirs = r.dirs;
    } catch (e) {
      // La carpeta guardada ya no existe: se sube hasta una que sí.
      const up = p.replace(/\/[^/]+\/?$/, '');
      if (fallback && up && up !== p) return go(up, true);
      error = t('folder.error', { error: e });
    } finally {
      loading = false;
    }
  }

  const crumbs = $derived.by(() => {
    if (!path.startsWith(ROOT)) return [{ label: path, path }];
    const out = [{ label: t('folder.storage'), path: ROOT }];
    let acc = ROOT;
    for (const part of path.slice(ROOT.length).split('/').filter(Boolean)) {
      acc += `/${part}`;
      out.push({ label: part, path: acc });
    }
    return out;
  });

  async function newFolder() {
    const r = await ask({ title: t('folder.newFolder'), icon: 'folder-plus', input: { label: t('folder.name') }, confirmLabel: t('common.add') });
    const name = r.ok ? r.value.trim().replace(/[\\/:*?"<>|]/g, '_') : '';
    if (!name) return;
    try {
      await invoke('make_dir', { path: `${path}/${name}` });
      go(`${path}/${name}`);
    } catch (e) {
      toast('error', String(e));
    }
  }

  function close(result) {
    const r = req;
    ui.folderPicker = null;
    opened = null;
    r?.resolve(result);
  }
</script>

{#if req}
  <Modal title={req.title || t('folder.title')} icon="folder" width={520} onclose={() => close(null)}>
    <div class="crumbs">
      {#each crumbs as c, i (c.path)}
        {#if i}<Icon name="chevron-right" size={12} />{/if}
        <button class="crumb" class:cur={i === crumbs.length - 1} onclick={() => go(c.path)}>{c.label}</button>
      {/each}
    </div>

    <div class="list card">
      {#if parent}
        <button class="row up" onclick={() => go(parent)}>
          <span class="r-icon"><Icon name="arrow-up" size={16} /></span>
          <span class="truncate">{t('folder.up')}</span>
        </button>
      {/if}
      {#if error}
        <div class="alert err msg"><Icon name="alert-triangle" size={16} /> {error}</div>
      {:else if !dirs.length && !loading}
        <div class="empty muted" in:fade={{ duration: d(150) }}>{t('folder.empty')}</div>
      {/if}
      {#each dirs as name (name)}
        <button class="row" onclick={() => go(`${path}/${name}`)}>
          <span class="r-icon"><Icon name="folder" size={16} /></span>
          <span class="truncate">{name}</span>
          <Icon name="chevron-right" size={14} />
        </button>
      {/each}
    </div>

    {#snippet footer()}
      <button class="btn ghost" onclick={newFolder} disabled={!!error}><Icon name="folder-plus" size={16} /> {t('folder.newFolder')}</button>
      <span class="spacer"></span>
      <button class="btn primary" onclick={() => close(path)} disabled={!!error || !path}><Icon name="check" size={16} /> {t('folder.use')}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .crumbs { display: flex; flex-wrap: wrap; align-items: center; gap: 2px; color: var(--muted); margin-bottom: 12px; }
  .crumb {
    border: none; background: transparent; color: var(--text-2);
    font-size: 13px; font-weight: 600; padding: 4px 6px; border-radius: 7px; cursor: pointer;
    max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .crumb:hover { background: var(--panel-3); color: var(--text); }
  .crumb.cur { color: var(--accent-2); }
  .list { max-height: min(46vh, 420px); overflow-y: auto; padding: 4px; }
  .row {
    width: 100%; height: 46px;
    display: flex; align-items: center; gap: 12px;
    padding: 0 12px; border: none; border-radius: 10px;
    background: transparent; color: var(--text); text-align: left; cursor: pointer;
    transition: background 0.15s;
  }
  .row:hover, .row:active { background: var(--panel-3); }
  .row > :global(svg:last-child) { color: var(--muted); margin-left: auto; flex-shrink: 0; }
  .row.up { color: var(--text-2); }
  .r-icon { width: 30px; height: 30px; border-radius: 8px; display: grid; place-items: center; background: rgba(var(--accent-rgb), 0.12); color: var(--accent-2); flex-shrink: 0; }
  .row.up .r-icon { background: var(--panel-3); color: var(--text-2); }
  .empty { padding: 22px 12px; text-align: center; font-size: 13px; }
  .msg { margin: 6px; }
</style>
