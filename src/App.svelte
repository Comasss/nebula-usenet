<script>
  import { t } from './lib/i18n.svelte.js';
  import { onMount } from 'svelte';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import TitleBar from './components/TitleBar.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import Toasts from './components/Toasts.svelte';
  import Dialog from './components/Dialog.svelte';
  import TransferDialog from './components/TransferDialog.svelte';
  import { tr, cancelScan } from './lib/transfer.svelte.js';
  import AddDialog from './components/AddDialog.svelte';
  import SetupWizard from './components/SetupWizard.svelte';
  import ClipboardPrompt from './components/ClipboardPrompt.svelte';
  import DropOverlay from './components/DropOverlay.svelte';
  import FolderPicker from './components/FolderPicker.svelte';
  import MobileHeader from './components/MobileHeader.svelte';
  import MobileNav from './components/MobileNav.svelte';
  import ViaMenu from './components/ViaMenu.svelte';
  import ActionSheet from './components/ActionSheet.svelte';
  import MobileSettings from './views/MobileSettings.svelte';
  import UpdatePrompt from './components/UpdatePrompt.svelte';
  import { initUpdater } from './lib/updater.svelte.js';
  import { installRipples } from './lib/feedback.js';
  import Icon from './components/Icon.svelte';
  import { invoke } from './lib/transport.js';
  import { isAndroid } from './lib/platform.js';
  import { onBackButtonPress } from '@tauri-apps/api/app';
  import Downloads from './views/Downloads.svelte';
  import History from './views/History.svelte';
  import Servers from './views/Servers.svelte';
  import Settings from './views/Settings.svelte';
  import Logs from './views/Logs.svelte';
  import Search from './views/Search.svelte';
  import Indexers from './views/Indexers.svelte';
  import Debrid from './views/Debrid.svelte';
  import Stats from './views/Stats.svelte';
  import { ui, init, queueAdd, toast, d } from './lib/state.svelte.js';
  import RemoteBanner from './components/RemoteBanner.svelte';
  import { remote } from './lib/remote.svelte.js';

  const VIEWS = { search: Search, downloads: Downloads, history: History, stats: Stats, indexers: Indexers, servers: Servers, debrid: Debrid, settings: Settings, logs: Logs };
  const ORDER = ['search', 'downloads', 'history', 'stats', 'indexers', 'servers', 'debrid', 'settings', 'logs'];
  const NEEDS = { search: 'featureIndexers', indexers: 'featureIndexers', downloads: 'featureNntp', history: 'featureNntp', servers: 'featureNntp', debrid: 'featureNntp' };

  // Si se desactiva la función de la vista actual, se salta a la primera disponible.
  $effect(() => {
    const s = ui.settings;
    if (!s) return;
    const need = NEEDS[ui.view];
    // Controlando un PC desde el móvil, Descargas e Historial son las del PC.
    if (remote.active && (ui.view === 'downloads' || ui.view === 'history')) return;
    if (need && !s[need]) ui.view = s.featureIndexers ? 'search' : 'downloads';
  });
  let prevIndex = 0;
  let direction = $state(1);

  $effect(() => {
    const i = ORDER.indexOf(ui.view);
    direction = i >= prevIndex ? 1 : -1;
    prevIndex = i;
  });

  $effect(() => {
    if (!ui.settings) return;
    document.documentElement.dataset.accent = ui.settings.accent || 'nebula';
    document.documentElement.dataset.anim = ui.settings.animations ? 'on' : 'off';
  });

  async function grantStorage() {
    ui.storage = await invoke('request_storage_access');
  }
  let storageLater = $state(false);

  onMount(() => {
    let unlistenDrop;
    installRipples();
    init()
      .then(initUpdater)
      .catch((e) => {
        console.error(e);
        toast('error', t('app.startError', { error: e }));
      });
    if (!isAndroid) getCurrentWebview()
      .onDragDropEvent((ev) => {
        const p = ev.payload;
        if (p.type === 'enter' || p.type === 'over') {
          ui.dragging = true;
        } else if (p.type === 'leave') {
          ui.dragging = false;
        } else if (p.type === 'drop') {
          ui.dragging = false;
          const nzbs = (p.paths ?? []).filter((x) => /\.nzb(\.gz)?$/i.test(x));
          if (nzbs.length) queueAdd(nzbs.map((value) => ({ kind: 'file', value })));
          else toast('warn', t('app.onlyNzb'));
        }
      })
      .then((f) => (unlistenDrop = f));

    // Botón Atrás de Android: cierra lo que esté abierto, luego vuelve a la vista principal y,
    // desde ella, deja Nebula en segundo plano (sin cerrarla, para no cortar las descargas).
    let backListener;
    if (isAndroid) {
      onBackButtonPress(() => {
        const home = ui.settings?.featureIndexers ? 'search' : 'downloads';
        if (tr.scanning) cancelScan();
        else if (ui.viaMenu) ui.viaMenu.resolve(null);
        else if (ui.actionSheet) ui.actionSheet.resolve(null);
        else if (document.querySelector('[role="dialog"], [role="listbox"]')) window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
        else if (ui.view === 'settings' && ui.settingsPage) ui.settingsPage = null;
        else if (ui.view !== home) ui.view = home;
        else invoke('app_background');
      })
        .then((l) => (backListener = l))
        .catch(console.error);
    }

    const onKey = (e) => {
      if (e.ctrlKey && e.key.toLowerCase() === 'r') e.preventDefault();
      if (e.key === 'F5') e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => {
      unlistenDrop?.();
      backListener?.unregister();
      window.removeEventListener('keydown', onKey);
    };
  });

  // En el móvil, indexadores, servidores, debrid y el registro se abren dentro de Configuración.
  const IN_SETTINGS = ['indexers', 'servers', 'debrid', 'logs'];
  $effect(() => {
    if (isAndroid && IN_SETTINGS.includes(ui.view)) {
      ui.settingsPage = ui.view;
      ui.view = 'settings';
    }
  });

  // La barra inferior se reduce al bajar por una lista y vuelve al subir.
  let lastScroll = 0;
  function onPageScroll(e) {
    if (!isAndroid) return;
    const el = e.target;
    if (!(el instanceof HTMLElement) || el.scrollHeight - el.clientHeight < 120) return;
    const y = el.scrollTop;
    if (y < 40) ui.navCompact = false;
    else if (y > lastScroll + 6) ui.navCompact = true;
    else if (y < lastScroll - 10) ui.navCompact = false;
    lastScroll = y;
  }
  $effect(() => {
    ui.view;
    ui.settingsPage;
    ui.navCompact = false;
    lastScroll = 0;
  });

  const View = $derived(isAndroid && ui.view === 'settings' ? MobileSettings : (VIEWS[ui.view] ?? Downloads));
</script>

<div class="app">
  {#if isAndroid}
    <MobileHeader />
  {:else}
    <TitleBar />
  {/if}
  <div class="body">
    {#if !isAndroid}<Sidebar />{/if}
    <main>
      {#if isAndroid && ui.ready && remote.active && ['downloads', 'history', 'stats'].includes(ui.view)}
        <RemoteBanner />
      {:else if isAndroid && ui.ready && !ui.storage && !storageLater}
        <div class="storage alert warn">
          <Icon name="folder" size={18} />
          <div class="spacer">
            <strong>{t('mobile.storageTitle')}</strong>
            <div>{t('mobile.storageText')}</div>
            <div class="st-actions">
              <button class="btn sm primary" onclick={grantStorage}>{t('mobile.storageGrant')}</button>
              <button class="btn sm ghost" onclick={() => (storageLater = true)}>{t('mobile.storageLater')}</button>
            </div>
          </div>
        </div>
      {/if}
      {#if ui.ready}
        {#key ui.view}
          <div class="page" onscrollcapture={onPageScroll} in:fly={{ y: 14 * direction, duration: d(320), easing: cubicOut, opacity: 0 }}>
            <View />
          </div>
        {/key}
      {:else}
        <div class="boot"><div class="spinner"></div></div>
      {/if}
    </main>
  </div>
  {#if isAndroid && ui.ready}<MobileNav />{/if}
</div>

{#if ui.ready}
  <ClipboardPrompt />
  <AddDialog />
  {#if ui.setupOpen && ui.settings}
    <SetupWizard />
  {/if}
  <FolderPicker />
  <ViaMenu />
  <ActionSheet />
  <UpdatePrompt />
  <TransferDialog />
  <Dialog />
{/if}
<DropOverlay />
<Toasts />

<style>
  .app { height: 100%; display: flex; flex-direction: column; }
  .body { flex: 1; display: flex; min-height: 0; }
  main {
    flex: 1;
    min-width: 0;
    position: relative;
    display: grid;
    background:
      radial-gradient(900px 500px at 85% -10%, rgba(var(--accent-rgb), 0.09), transparent 60%),
      radial-gradient(700px 400px at 0% 110%, rgba(var(--accent-rgb), 0.05), transparent 60%),
      var(--bg-2);
    border-top-left-radius: 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.04);
    border-left: 1px solid rgba(255, 255, 255, 0.04);
    overflow: hidden;
  }
  .page { grid-area: 2 / 1; overflow-y: auto; min-height: 0; height: 100%; }
  :global(html.mobile) .page { padding-bottom: var(--nav-space); scroll-padding-bottom: var(--nav-space); }
  main { grid-template-rows: auto minmax(0, 1fr); }
  .boot { grid-area: 2 / 1; }
  .storage { grid-area: 1 / 1; margin: 12px 12px 0; }
  .st-actions { display: flex; gap: 8px; margin-top: 10px; }
  :global(html.mobile) main { border-radius: 0; border: none; }
  .boot { display: grid; place-items: center; }
  .spinner { width: 34px; height: 34px; border-radius: 50%; border: 3px solid var(--panel-3); border-top-color: var(--accent); animation: spin 0.8s linear infinite; }
</style>
