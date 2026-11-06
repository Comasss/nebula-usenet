<script>
  import { t, th, LANGUAGES, setLanguage, systemLanguage, locale } from '../lib/i18n.svelte.js';
  import { invoke, listen } from '../lib/transport.js';
  import { pickFolder, openUrl, isAndroid, isServer, isWindows, isDesktopApp } from '../lib/platform.js';
  import { fly, scale } from 'svelte/transition';
  import Icon from '../components/Icon.svelte';
  import SettingRow from '../components/SettingRow.svelte';
  import Select from '../components/Select.svelte';
  import { ui, saveSettings, toast, d } from '../lib/state.svelte.js';
  import { usableDebrid, debridLabel, hasNntp } from '../lib/debrid.js';
  import ProgressBar from '../components/ProgressBar.svelte';
  import RemoteSettings from '../components/RemoteSettings.svelte';
  import * as f from '../lib/format.js';
  import { upd, checkUpdates, startUpdate, cancelDownload } from '../lib/updater.svelte.js';
  import { setContext, tick } from 'svelte';
  import { showSend, showReceive, showPaste, scanCode } from '../lib/transfer.svelte.js';
  import FinishActions from '../components/FinishActions.svelte';
  import Categories from '../components/Categories.svelte';

  // `only`: muestra solo esa sección (el móvil abre cada sección como una página).
  // `query`: búsqueda que llega de fuera (la portada de Ajustes del móvil); sin ella se usa el buscador propio.
  let { only = null, query = null } = $props();
  // Secciones que no existen aquí: en el navegador (nebula-server) no hay portapapeles que vigilar,
  // ventana, bandeja ni actualizador; la cuenta del servidor solo existe allí.
  const NOT_HERE = isServer ? ['links', 'windows', 'updates'] : ['server'];
  const show = (id) => !NOT_HERE.includes(id) && (!only || only === id);

  // ---- Buscador ----
  let localQuery = $state('');
  const q = $derived(f.fold(query ?? localQuery));
  setContext('settings-search', { get q() { return q; } });
  let contentEl = $state(null);
  let noResults = $state(false);

  function titleOf(id) {
    if (id === 'debrid') return t('settings.downloadMethod');
    if (id === 'search') return t('settings.searchTitle');
    if (id === 'windows') return isAndroid ? t('mobile.appTitle') : t('settings.windowsTitle');
    if (id === 'api') return t('api.title');
    if (id === 'remote') return isAndroid ? t('remote.mobileTitle') : t('settings.section.remote');
    if (id === 'server') return t('web.section');
    return t(`settings.section.${id}`);
  }
  // Si coincide el título de la sección, se muestra entera.
  const secHit = (id) => !!q && f.fold(titleOf(id)).includes(q);

  $effect(() => {
    if (!q || !contentEl) {
      noResults = false;
      return;
    }
    tick().then(() => {
      noResults = ![...contentEl.querySelectorAll('section')].some((el) => getComputedStyle(el).display !== 'none');
    });
  });

  function onSearch(e) {
    localQuery = e.target.value;
    if (contentEl) contentEl.scrollTop = 0;
  }

  const s = $derived(ui.settings);
  const debridList = $derived(usableDebrid(s));
  let newPattern = $state('');

  const SECTIONS = [
    { id: 'features', icon: 'sparkles' },
    { id: 'language', icon: 'globe' },
    { id: 'debrid', icon: 'cloud' },
    { id: 'search', icon: 'search' },
    { id: 'folders', icon: 'folder' },
    { id: 'categories', icon: 'tag' },
    { id: 'post', icon: 'package' },
    { id: 'links', icon: 'clipboard' },
    { id: 'windows', icon: isAndroid ? 'smartphone' : 'monitor' },
    { id: 'server', icon: 'server' },
    { id: 'finish', icon: 'bell' },
    { id: 'network', icon: 'wifi' },
    { id: 'look', icon: 'palette' },
    { id: 'api', icon: 'plug' },
    { id: 'remote', icon: isAndroid ? 'monitor' : 'smartphone' },
    { id: 'transfer', icon: 'qr' },
    { id: 'updates', icon: 'refresh' },
    { id: 'about', icon: 'info' },
  ].filter((sec) => !NOT_HERE.includes(sec.id));
  let section = $state('features');

  function setFeature(key, v) {
    const other = key === 'featureNntp' ? 'featureIndexers' : 'featureNntp';
    if (!v && !s[other]) {
      toast('warn', t('settings.keepOneFeature'));
      return;
    }
    saveSettings({ [key]: v });
  }

  const ACCENTS = [
    { id: 'nebula', a: '#7c5cff', b: '#22d3ee' },
    { id: 'ocean', a: '#3b82f6', b: '#2dd4bf' },
    { id: 'emerald', a: '#10b981', b: '#a3e635' },
    { id: 'sunset', a: '#f97316', b: '#ec4899' },
    { id: 'rose', a: '#ec4899', b: '#8b5cf6' },
  ];

  const systemLang = LANGUAGES.find((l) => l.code === systemLanguage())?.name ?? 'English';

  async function setLang(code) {
    if (await saveSettings({ language: code })) setLanguage(code);
  }

  function set(key) {
    return (v) => saveSettings({ [key]: v });
  }

  async function pick(key) {
    const dir = await pickFolder({ defaultPath: s[key] || undefined });
    if (dir) saveSettings({ [key]: dir });
  }

  function num(key, min, max) {
    return (e) => {
      let v = Math.round(Number(String(e.target.value).replace(',', '.')));
      if (isNaN(v)) v = min;
      v = Math.max(min, Math.min(max, v));
      e.target.value = v;
      saveSettings({ [key]: v });
    };
  }

  function addPattern() {
    const p = newPattern.trim();
    if (!p || s.clipboardPatterns.includes(p)) return;
    saveSettings({ clipboardPatterns: [...s.clipboardPatterns, p] });
    newPattern = '';
  }

  async function resetPatterns() {
    saveSettings({ clipboardPatterns: await invoke('default_patterns') });
  }

  // ---- API Newznab ----
  const DOCS_URL = 'https://github.com/Comasss/nebula-usenet/blob/main/docs/API.md';
  const SAB_DOCS_URL = 'https://github.com/Comasss/nebula-usenet/blob/main/docs/SABNZBD.md';
  // Host y puerto para Sonarr/Radarr. En nebula-server, los de esta misma página (las APIs
  // responden en el mismo puerto); en la app, la IP de la red local si está permitida o localhost.
  const webPort = location.port || (location.protocol === 'https:' ? '443' : '80');
  const sabPort = $derived(isServer ? webPort : s?.apiPort);
  const sabHost = $derived.by(() => {
    if (isServer) return location.hostname;
    const url = apiStatus?.urls?.[apiStatus.urls.length - 1] ?? `http://localhost:${s?.apiPort ?? 5077}/api`;
    try {
      return new URL(url).hostname;
    } catch {
      return 'localhost';
    }
  });
  let apiStatus = $state(null);
  let showKey = $state(false);
  const apiIndexers = $derived((s?.indexers ?? []).filter((i) => i.enabled).length);

  $effect(() => {
    if (!show('api')) return;
    let off = null;
    invoke('api_status').then((st) => (apiStatus = st)).catch(() => {});
    listen('api-status', (e) => (apiStatus = e.payload)).then((fn) => (off = fn));
    return () => off?.();
  });

  function newKey() {
    const b = crypto.getRandomValues(new Uint8Array(16));
    return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
  }

  // ---- nebula-server: herramientas y cuenta ----
  let tools = $state([]);
  async function loadTools() {
    if (isWindows || isAndroid) return;
    tools = await invoke('tools_status').catch(() => []);
  }
  loadTools();

  let account = $state({ user: '', current: '', next: '' });
  if (isServer) {
    fetch('api/v1/auth', { credentials: 'same-origin' })
      .then((r) => r.json())
      .then((v) => (account.user = v.user ?? ''))
      .catch(() => {});
  }
  async function logout() {
    await fetch('api/v1/auth/logout', { method: 'POST', credentials: 'same-origin' }).catch(() => {});
    location.reload();
  }
  async function changePassword(e) {
    e.preventDefault();
    try {
      await invoke('auth_change', { current: account.current, password: account.next });
      account.current = account.next = '';
      toast('success', t('web.passwordChanged'));
    } catch (err) {
      toast('error', String(err));
    }
  }

  function setApi(v) {
    saveSettings(v && !s.apiKey ? { apiEnabled: true, apiKey: newKey() } : { apiEnabled: v });
  }

  async function regenerateKey() {
    if (await saveSettings({ apiKey: newKey() })) toast('success', t('api.regenerated'));
  }

  async function copy(text) {
    try {
      if (navigator.clipboard && window.isSecureContext) await navigator.clipboard.writeText(text);
      else {
        // Por http en la red local el navegador no da acceso al portapapeles: se copia a la antigua.
        const ta = Object.assign(document.createElement('textarea'), { value: text });
        document.body.append(ta);
        ta.select();
        const ok = document.execCommand('copy');
        ta.remove();
        if (!ok) throw new Error('copy');
      }
      toast('success', t('api.copied'), 2500);
    } catch (e) {
      toast('error', String(e));
    }
  }

  const updInfo = $derived(upd.info);
  const updDesc = $derived.by(() => {
    if (upd.phase === 'downloading') return t('update.downloadingPct', { pct: Math.round(upd.total ? (upd.downloaded / upd.total) * 100 : 0) });
    if (upd.phase === 'error') return upd.error;
    if (updInfo?.available) return t('update.availableDesc');
    if (upd.checking) return t('update.checking');
    if (upd.checkedAt) return t('update.upToDateDesc', { time: new Date(upd.checkedAt).toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' }) });
    return t('update.neverChecked');
  });

  function scrollTo(id) {
    section = id;
    document.getElementById(`sec-${id}`)?.scrollIntoView({ behavior: s.animations ? 'smooth' : 'auto', block: 'start' });
  }

  function onScroll(e) {
    const top = e.currentTarget.getBoundingClientRect().top;
    let cur = SECTIONS[0].id;
    for (const sec of SECTIONS) {
      const el = document.getElementById(`sec-${sec.id}`);
      if (el && el.getBoundingClientRect().top - top < 140) cur = sec.id;
    }
    section = cur;
  }
</script>

{#if s}
  <div class="wrap" class:single={!!only} class:embedded={query !== null} class:searching={!!q}>
    {#if !only && query === null}
    <aside class="toc">
      <h1>{t('nav.settings')}</h1>
      <p class="saved"><Icon name="check" size={13} /> {t('settings.autosave')}</p>
      <label class="find">
        <Icon name="search" size={15} />
        <input type="search" placeholder={t('settings.searchPlaceholder')} value={localQuery} oninput={onSearch} onkeydown={(e) => e.key === 'Escape' && onSearch({ target: { value: '' } })} />
      </label>
      <nav>
        {#each SECTIONS as sec}
          <button class:sel={section === sec.id} onclick={() => scrollTo(sec.id)}>
            <Icon name={sec.icon} size={16} /> {sec.id === 'windows' && isAndroid ? t('mobile.appTitle') : sec.id === 'remote' || sec.id === 'server' ? titleOf(sec.id) : t(`settings.section.${sec.id}`)}
          </button>
        {/each}
      </nav>
    </aside>
    {/if}

    <div class="content" onscroll={onScroll} bind:this={contentEl}>
      {#if noResults}
        <div class="empty"><Icon name="search" size={22} /> {t('settings.noResults', { q: query ?? localQuery })}</div>
      {/if}
      {#if show('features')}
      <section id="sec-features" class:hit={secHit('features')}>
        <h2><Icon name="sparkles" size={18} /> {t('settings.section.features')}</h2>
        <div class="card group">
          <SettingRow icon="search" label={t('nav.indexers')} description={t('settings.featIndexersDesc')} checked={s.featureIndexers} onchange={(v) => setFeature('featureIndexers', v)} />
          <SettingRow icon="server" label={t('settings.featNntp')} description={t('settings.featNntpDesc')} checked={s.featureNntp} onchange={(v) => setFeature('featureNntp', v)} />
        </div>
      </section>
      {/if}

      {#if show('language')}
      <section id="sec-language" class:hit={secHit('language')}>
        <h2><Icon name="globe" size={18} /> {t('settings.section.language')}</h2>
        <div class="card group">
          <SettingRow icon="globe" label={t('settings.language')} description={t('settings.languageDesc')}>
            <Select class="lang" value={s.language || ''} label={t('settings.language')} onchange={setLang}
              options={[{ value: '', label: t('settings.languageAuto', { lang: systemLang }) }, ...LANGUAGES.map((l) => ({ value: l.code, label: l.name }))]} />
          </SettingRow>
        </div>
      </section>
      {/if}

      {#if show('debrid')}
      <section id="sec-debrid" class:hit={secHit('debrid')} class:dim={!s.featureNntp}>
        <h2><Icon name="cloud" size={18} /> {t('settings.downloadMethod')}</h2>
        <div class="card group">
          <SettingRow
            icon="download"
            label={t('settings.downloadWith')}
            description={debridList.length
              ? t(debridList.length > 1 ? 'settings.downloadWithDescMany' : 'settings.downloadWithDesc', { name: debridLabel(debridList[0]) })
              : t('settings.downloadWithNone')}>
            <div class="seg">
              <button class:sel={s.downloadVia !== 'debrid'} onclick={() => saveSettings({ downloadVia: 'nntp' })}><Icon name="server" size={14} /> Usenet (NNTP)</button>
              <button class:sel={s.downloadVia === 'debrid'} disabled={!debridList.length} onclick={() => saveSettings({ downloadVia: 'debrid' })}><Icon name="cloud" size={14} /> Debrid</button>
            </div>
          </SettingRow>
          {#if debridList.length && !hasNntp(s) && s.downloadVia !== 'debrid'}
            <div class="note"><Icon name="info" size={14} /> {t('settings.noNntpNote', { name: debridLabel(debridList[0]) })}</div>
          {/if}
          <SettingRow label={t('settings.fallbackNntp')} description={t('settings.fallbackNntpDesc')} checked={s.debridFallbackNntp} onchange={set('debridFallbackNntp')} />
          <SettingRow label={t('settings.debridConns')} description={t('settings.debridConnsDesc')}>
            <input class="input small" type="number" min="1" max="32" value={s.debridConnections} onchange={num('debridConnections', 1, 32)} />
          </SettingRow>
          <SettingRow label={t('settings.debridDelete')} description={t('settings.debridDeleteDesc')} checked={s.debridDeleteAfter} onchange={set('debridDeleteAfter')} />
          <SettingRow icon="cloud" label={t('settings.debridServices')} description={debridList.length ? t('settings.debridOrder', { list: debridList.map(debridLabel).join(', ') }) : t('settings.debridNone')}>
            <button class="btn sm" onclick={() => (ui.view = 'debrid')}>{t('settings.manage')} <Icon name="chevron-right" size={14} /></button>
          </SettingRow>
        </div>
      </section>
      {/if}

      {#if show('search')}
      <section id="sec-search" class:hit={secHit('search')} class:dim={!s.featureIndexers}>
        <h2><Icon name="search" size={18} /> {t('settings.searchTitle')}</h2>
        <div class="card group">
          <SettingRow icon="layers" label={t('settings.dedup')} description={t('settings.dedupDesc')} checked={s.searchDedup} onchange={set('searchDedup')} />
          <SettingRow icon="zap" label={t('settings.quickAdd')} description={t('settings.quickAddDesc')} checked={s.searchQuickAdd} onchange={set('searchQuickAdd')} disabled={!s.featureNntp} />
          <SettingRow label={t('settings.searchLimit')} description={t('settings.searchLimitDesc')}>
            <input class="input small" type="number" min="10" max="500" value={s.searchLimit} onchange={num('searchLimit', 10, 500)} />
          </SettingRow>
          <SettingRow label={t('settings.nzbDir')} description={t('settings.nzbDirDesc')}>
            <input class="input path" value={s.nzbSaveDir} onchange={(e) => saveSettings({ nzbSaveDir: e.target.value })} />
            <button class="btn sm" onclick={() => pick('nzbSaveDir')}>{t('common.browse')}</button>
          </SettingRow>
          <SettingRow label={t('settings.nzbAsk')} checked={s.nzbAskSaveDir} onchange={set('nzbAskSaveDir')} />
          <SettingRow icon="layers" label={t('nav.indexers')} description={t('settings.indexersDesc')}>
            <button class="btn sm" onclick={() => (ui.view = 'indexers')}>{t('settings.manage')} <Icon name="chevron-right" size={14} /></button>
          </SettingRow>
        </div>
      </section>
      {/if}

      {#if show('folders')}
      <section id="sec-folders" class:hit={secHit('folders')}>
        <h2><Icon name="folder" size={18} /> {t('settings.section.folders')}</h2>
        <div class="card group">
          <SettingRow label={t('settings.downloadDir')} description={t('settings.downloadDirDesc')}>
            <input class="input path" value={s.downloadDir} onchange={(e) => saveSettings({ downloadDir: e.target.value })} />
            <button class="btn sm" onclick={() => pick('downloadDir')}>{t('common.browse')}</button>
          </SettingRow>
          <SettingRow label={t('settings.tempDir')} description={t('settings.tempDirDesc')}>
            <input class="input path" value={s.incompleteDir} placeholder={t('settings.automatic')} onchange={(e) => saveSettings({ incompleteDir: e.target.value })} />
            <button class="btn sm" onclick={() => pick('incompleteDir')}>{t('common.browse')}</button>
          </SettingRow>
          <SettingRow label={t('settings.askDestination')} description={t('settings.askDestinationDesc')} checked={s.askDestination} onchange={set('askDestination')} />
          <SettingRow label={t('settings.jobFolder')} description={t('settings.jobFolderDesc')} checked={s.createJobFolder} onchange={set('createJobFolder')} />
          <SettingRow label={t('add.paused')} description={t('settings.addPausedDesc')} checked={s.addPaused} onchange={set('addPaused')} />
        </div>
      </section>
      {/if}

      {#if show('categories')}
      <section id="sec-categories" class:hit={secHit('categories')}>
        <h2><Icon name="tag" size={18} /> {t('settings.section.categories')}</h2>
        <Categories />
      </section>
      {/if}

      {#if show('post')}
      <section id="sec-post" class:hit={secHit('post')}>
        <h2><Icon name="package" size={18} /> {t('settings.section.post')}</h2>
        <div class="card group">
          <SettingRow icon="shield" label={t('settings.par2')} description={t('settings.par2Desc')} checked={s.par2Enabled} onchange={set('par2Enabled')} />
          <SettingRow icon="archive" label={t('settings.extract')} description={t('settings.extractDesc')} checked={s.extractEnabled} onchange={set('extractEnabled')} />
          <SettingRow label={t('settings.deleteArchives')} description={t('settings.deleteArchivesDesc')} checked={s.deleteArchives} onchange={set('deleteArchives')} disabled={!s.extractEnabled} />
          <SettingRow label={t('settings.deletePar2')} checked={s.deletePar2} onchange={set('deletePar2')} />
          <SettingRow icon="zap" label={t('settings.abortHopeless')} description={t('settings.abortHopelessDesc')} checked={s.abortHopeless} onchange={set('abortHopeless')} />
          {#if !isWindows && !isAndroid}
            {#each tools as tl (tl.tool)}
              <SettingRow label={tl.tool === 'par2' ? t('web.par2Path') : t('web.sevenZipPath')} description={tl.found ? t('web.toolFound', { path: tl.path }) : t('web.toolMissing', { path: tl.path })}>
                {#if !tl.found}<span class="chip warn"><Icon name="alert-triangle" size={11} /> {t('web.notFound')}</span>{/if}
                <input class="input path" value={tl.tool === 'par2' ? s.par2Path : s.sevenZipPath} placeholder={t('settings.automatic')} spellcheck="false"
                  onchange={async (e) => { await saveSettings({ [tl.tool === 'par2' ? 'par2Path' : 'sevenZipPath']: e.target.value.trim() }); loadTools(); }} />
              </SettingRow>
            {/each}
          {/if}
        </div>
      </section>
      {/if}

      {#if show('links')}
      <section id="sec-links" class:hit={secHit('links')}>
        <h2><Icon name="clipboard" size={18} /> {t('settings.section.links')}</h2>
        <div class="card group">
          <SettingRow icon="clipboard" label={t('settings.clipboard')} description={t('settings.clipboardDesc')} checked={s.clipboardMonitor} onchange={set('clipboardMonitor')} />
          <SettingRow label={t('settings.clipboardInspect')} description={t('settings.clipboardInspectDesc')} checked={s.clipboardInspect} onchange={set('clipboardInspect')} disabled={!s.clipboardMonitor} />
          {#if !isAndroid}
            <SettingRow label={t('settings.clipboardShow')} description={t('settings.clipboardShowDesc')} checked={s.clipboardShowWindow} onchange={set('clipboardShowWindow')} disabled={!s.clipboardMonitor} />
          {/if}
          <div class="patterns" class:disabled={!s.clipboardMonitor}>
            <div class="p-head">
              <div>
                <div class="p-title">{t('settings.patterns')}</div>
                <div class="p-desc">{@html th('settings.patternsDesc')}</div>
              </div>
              <button class="btn ghost sm" onclick={resetPatterns}><Icon name="refresh" size={13} /> {t('settings.restore')}</button>
            </div>
            <div class="chips">
              {#each s.clipboardPatterns as p (p)}
                <span class="pchip" in:scale={{ start: 0.7, duration: d(200) }}>
                  <code>{p}</code>
                  <button onclick={() => saveSettings({ clipboardPatterns: s.clipboardPatterns.filter((x) => x !== p) })} aria-label={t('settings.removePattern')}><Icon name="x" size={12} /></button>
                </span>
              {/each}
              <form class="p-add" onsubmit={(e) => { e.preventDefault(); addPattern(); }}>
                <input class="input" placeholder={t('settings.addPattern')} bind:value={newPattern} />
              </form>
            </div>
          </div>
        </div>
      </section>
      {/if}

      {#if show('windows')}
      <section id="sec-windows" class:hit={secHit('windows')}>
        {#if isAndroid}
          <h2><Icon name="smartphone" size={18} /> {t('mobile.appTitle')}</h2>
        {:else}
          <h2><Icon name="monitor" size={18} /> {t('settings.windowsTitle')}</h2>
        {/if}
        <div class="card group">
          {#if isAndroid}
            <SettingRow icon="folder" label={t('mobile.storageTitle')} description={t('mobile.storageText')}>
              {#if ui.storage}
                <span class="chip ok"><Icon name="check" size={11} stroke={3} /> OK</span>
              {:else}
                <button class="btn sm primary" onclick={async () => (ui.storage = await invoke('request_storage_access'))}>{t('mobile.storageGrant')}</button>
              {/if}
            </SettingRow>
          {:else}
            <SettingRow icon="file" label={t('settings.associate')} description={t('settings.associateDesc')} checked={s.associateNzb} onchange={set('associateNzb')} />
            <SettingRow icon="power" label={t('settings.startWithWindows')} checked={s.startWithWindows} onchange={set('startWithWindows')} />
            <SettingRow label={t('settings.startMinimized')} checked={s.startMinimized} onchange={set('startMinimized')} />
            <SettingRow label={t('settings.minimizeToTray')} description={t('settings.minimizeToTrayDesc')} checked={s.minimizeToTray} onchange={set('minimizeToTray')} />
            <SettingRow label={t('settings.closeToTray')} description={t('settings.closeToTrayDesc')} checked={s.closeToTray} onchange={set('closeToTray')} />
          {/if}
          <SettingRow label={t('settings.notifyComplete')} checked={s.notifyComplete} onchange={set('notifyComplete')} />
          <SettingRow label={t('settings.notifyFailed')} checked={s.notifyFailed} onchange={set('notifyFailed')} />
          <SettingRow label={t('settings.confirmDelete')} checked={s.confirmDelete} onchange={set('confirmDelete')} />
        </div>
      </section>
      {/if}

      {#if show('server')}
      <section id="sec-server" class:hit={secHit('server')}>
        <h2><Icon name="server" size={18} /> {t('web.section')}</h2>
        <div class="card group">
          <SettingRow icon="user" label={t('web.account', { user: account.user })} description={t('web.accountDesc')}>
            <button class="btn sm" onclick={logout}><Icon name="log-out" size={14} /> {t('web.signOut')}</button>
          </SettingRow>
          <form class="pw" onsubmit={changePassword}>
            <div class="pw-title">{t('web.changePassword')}</div>
            <input class="input" type="password" placeholder={t('web.currentPassword')} bind:value={account.current} autocomplete="current-password" />
            <input class="input" type="password" placeholder={t('web.newPassword')} bind:value={account.next} autocomplete="new-password" />
            <button class="btn sm" type="submit" disabled={!account.current || !account.next}>{t('common.save')}</button>
          </form>
          <SettingRow label={t('settings.confirmDelete')} checked={s.confirmDelete} onchange={set('confirmDelete')} />
        </div>
      </section>
      {/if}

      {#if show('finish')}
      <section id="sec-finish" class:hit={secHit('finish')}>
        <h2><Icon name="bell" size={18} /> {t('settings.section.finish')}</h2>
        <FinishActions />
      </section>
      {/if}

      {#if show('network')}
      <section id="sec-network" class:hit={secHit('network')}>
        <h2><Icon name="wifi" size={18} /> {t('settings.section.network')}</h2>
        <div class="card group">
          <SettingRow icon="gauge" label={t('downloads.speedLimit')} description={t('settings.speedLimitDesc')}>
            <input class="input small" type="number" min="0" step="0.5" value={s.speedLimitKbps ? +(s.speedLimitKbps / 1024).toFixed(1) : 0}
              onchange={(e) => { const v = Math.max(0, parseFloat(String(e.target.value).replace(',', '.')) || 0); saveSettings({ speedLimitKbps: Math.round(v * 1024) }); invoke('set_speed_limit', { kbps: Math.round(v * 1024) }); }} />
            <span class="unit">MB/s</span>
          </SettingRow>
          <SettingRow label={t('settings.retries')} description={t('settings.retriesDesc')}>
            <input class="input small" type="number" min="0" max="10" value={s.maxRetries} onchange={num('maxRetries', 0, 10)} />
          </SettingRow>
          <SettingRow label={t('settings.timeout')} description={t('settings.timeoutDesc')}>
            <input class="input small" type="number" min="5" max="300" value={s.timeoutSecs} onchange={num('timeoutSecs', 5, 300)} />
            <span class="unit">s</span>
          </SettingRow>
          <SettingRow label={t('settings.historyLimit')} description={t('settings.historyLimitDesc')}>
            <input class="input small" type="number" min="10" max="5000" value={s.historyLimit} onchange={num('historyLimit', 10, 5000)} />
          </SettingRow>
          <SettingRow icon="server" label={t('settings.nntpServers')} description={t('settings.nntpServersDesc')}>
            <button class="btn sm" onclick={() => (ui.view = 'servers')}>{t('settings.manage')} <Icon name="chevron-right" size={14} /></button>
          </SettingRow>
        </div>
      </section>
      {/if}

      {#if show('look')}
      <section id="sec-look" class:hit={secHit('look')}>
        <h2><Icon name="palette" size={18} /> {t('settings.section.look')}</h2>
        <div class="card group">
          <SettingRow label={t('settings.accent')}>
            <div class="swatches">
              {#each ACCENTS as a}
                <button class="swatch" class:sel={s.accent === a.id} style="--a:{a.a}; --b:{a.b}" title={a.id === 'nebula' ? 'Nebula' : t(`settings.accentName.${a.id}`)} onclick={() => saveSettings({ accent: a.id })}>
                  {#if s.accent === a.id}<span in:scale={{ duration: d(200) }}><Icon name="check" size={14} stroke={3} /></span>{/if}
                </button>
              {/each}
            </div>
          </SettingRow>
          <SettingRow icon="sparkles" label={t('settings.animations')} description={t('settings.animationsDesc')} checked={s.animations} onchange={set('animations')} />
          {#if isAndroid}
            <SettingRow icon="smartphone" label={t('settings.haptics')} description={t('settings.hapticsDesc')} checked={s.haptics !== false} onchange={set('haptics')} />
          {/if}
          <SettingRow icon="rocket" label={t('settings.wizard')} description={t('settings.wizardDesc')}>
            <button class="btn sm" onclick={() => (ui.setupOpen = true)}>{t('common.open')}</button>
          </SettingRow>
        </div>
      </section>
      {/if}

      {#if show('api')}
      <section id="sec-api" class:hit={secHit('api')}>
        <h2><Icon name="plug" size={18} /> {t('api.title')}</h2>
        <div class="card group">
          {#if isServer}
            <div class="note"><Icon name="info" size={14} /> <span>{t('web.apiNote', { url: `${location.origin}/api` })}</span></div>
          {/if}
          <SettingRow icon="plug" label={t('api.enable')} description={t('api.enableDesc')} keywords={`API ${t('api.key')} ${t('api.port')} ${t('api.lan')} Newznab NZBHydra`} checked={!!s.apiEnabled} onchange={setApi} />
          {#if s.apiEnabled}
            {#if apiStatus?.error}
              <div class="note err"><Icon name="alert-triangle" size={14} /> {apiStatus.error}</div>
            {:else if !apiIndexers}
              <div class="note"><Icon name="info" size={14} /> {t('api.noIndexers')}</div>
            {/if}
            <SettingRow label={t('api.address')} description={t('api.addressDesc')}>
              <div class="urls">
                {#if apiStatus?.running}
                  {#each apiStatus.urls as url}
                    <span class="url"><code>{url}</code><button class="btn ghost sm icon" onclick={() => copy(url)} aria-label={t('api.copy')} title={t('api.copy')}><Icon name="copy" size={13} /></button></span>
                  {/each}
                {:else}
                  <span class="chip">{t('api.stopped')}</span>
                {/if}
              </div>
            </SettingRow>
            <SettingRow icon="key" label={t('api.key')} description={t('api.keyDesc')}>
              <input class="input key" readonly type={showKey ? 'text' : 'password'} value={s.apiKey} onfocus={(e) => e.target.select()} />
              <button class="btn ghost sm icon" onclick={() => (showKey = !showKey)} aria-label={t('common.showApiKey')} title={t('common.showApiKey')}><Icon name={showKey ? 'eye-off' : 'eye'} size={14} /></button>
              <button class="btn ghost sm icon" onclick={() => copy(s.apiKey)} aria-label={t('api.copy')} title={t('api.copy')}><Icon name="copy" size={14} /></button>
              <button class="btn sm" onclick={regenerateKey}><Icon name="refresh" size={13} /> {t('api.regenerate')}</button>
            </SettingRow>
            <SettingRow label={t('api.port')} description={t('api.portDesc')}>
              <input class="input small" type="number" min="1024" max="65535" value={s.apiPort} onchange={num('apiPort', 1024, 65535)} />
            </SettingRow>
            <SettingRow icon="wifi" label={t('api.lan')} description={t('api.lanDesc')} checked={!!s.apiLan} onchange={set('apiLan')} />
            <div class="sab">
              <div class="sab-title"><Icon name="download" size={15} /> {t('api.sab.title')}</div>
              <p>{t('api.sab.desc')}</p>
              <ol>
                <li>{t('api.sab.step1')}</li>
                <li>
                  {t('api.sab.step2')}
                  <div class="sab-grid">
                    <span>Host</span><code>{sabHost}</code>
                    <span>Port</span><code>{sabPort}</code>
                    <span>URL Base</span><code class="muted">{t('api.sab.empty')}</code>
                    <span>API Key</span><span class="url"><code>{showKey ? s.apiKey : '•'.repeat(12)}</code><button class="btn ghost sm icon" onclick={() => copy(s.apiKey)} aria-label={t('api.copy')} title={t('api.copy')}><Icon name="copy" size={13} /></button></span>
                    <span>Category</span><code>{(s.categories ?? []).map((c) => c.name).join(' / ') || 'tv / movies'}</code>
                  </div>
                </li>
                <li>{t('api.sab.step3')}</li>
              </ol>
              {#if !(s.categories ?? []).length}
                <div class="sab-warn"><Icon name="alert-triangle" size={14} /> <span>{t('api.sab.noCategories')}</span> <button class="btn sm" onclick={() => scrollTo('categories')}>{t('settings.section.categories')}</button></div>
              {/if}
              {#if !s.apiLan}
                <div class="sab-hint"><Icon name="info" size={13} /> {t('api.sab.lanHint')}</div>
              {/if}
              <button class="btn sm" onclick={() => openUrl(SAB_DOCS_URL)}><Icon name="external" size={13} /> {t('api.sab.docs')}</button>
            </div>
            {#if isAndroid}
              <div class="note"><Icon name="smartphone" size={14} /> {t('api.androidNote')}</div>
            {/if}
          {/if}
          <SettingRow icon="external" label={t('api.docs')} description={t('api.docsDesc')}>
            <button class="btn sm" onclick={() => openUrl(DOCS_URL)}>{t('common.open')} <Icon name="chevron-right" size={14} /></button>
          </SettingRow>
        </div>
      </section>
      {/if}

      {#if show('remote')}
      <section id="sec-remote" class:hit={secHit('remote')}>
        <h2><Icon name={isAndroid ? 'monitor' : 'smartphone'} size={18} /> {titleOf('remote')}</h2>
        <RemoteSettings />
      </section>
      {/if}

      {#if show('transfer')}
      <section id="sec-transfer" class:hit={secHit('transfer')}>
        <h2><Icon name="qr" size={18} /> {t('settings.section.transfer')}</h2>
        <div class="card group">
          {#if isAndroid}
            <SettingRow icon="scan" label={t('transfer.scanLabel')} description={t('transfer.scanDesc')}>
              <button class="btn sm primary" onclick={scanCode}><Icon name="scan" size={14} /> {t('transfer.scan')}</button>
            </SettingRow>
          {/if}
          <SettingRow icon="qr" label={t('transfer.sendLabel')} description={t('transfer.sendDesc')}>
            <button class="btn sm" onclick={showSend}>{t('transfer.showQr')}</button>
          </SettingRow>
          <SettingRow icon="wifi" label={t('transfer.receiveLabel')} description={t('transfer.receiveDesc')}>
            <button class="btn sm{isAndroid ? '' : ' primary'}" onclick={showReceive}>{t('transfer.showQr')}</button>
          </SettingRow>
          <SettingRow icon="clipboard" label={t('transfer.pasteLabel')} description={t('transfer.pasteDescShort')}>
            <button class="btn sm" onclick={showPaste}>{t('transfer.paste')}</button>
          </SettingRow>
        </div>
      </section>
      {/if}

      {#if show('updates')}
      <section id="sec-updates" class:hit={secHit('updates')}>
        <h2><Icon name="refresh" size={18} /> {t('settings.section.updates')}</h2>
        <div class="card group">
          <SettingRow
            icon="rocket"
            label={updInfo?.available ? t('update.availableLabel', { version: updInfo.version }) : t('update.currentLabel', { version: ui.info?.version })}
            description={updDesc}>
            {#if updInfo?.available}
              {#if upd.phase === 'downloading'}
                <button class="btn sm ghost" onclick={cancelDownload}>{t('common.cancel')}</button>
              {:else}
                <button class="btn sm ghost" onclick={() => (upd.open = true)}>{t('update.whatsNew')}</button>
              {/if}
              <button class="btn sm primary" onclick={startUpdate} disabled={upd.phase === 'downloading' || upd.phase === 'installing'}>
                <Icon name={updInfo.kind ? 'download' : 'external'} size={14} /> {updInfo.kind ? t('update.now') : t('update.openPage')}
              </button>
            {:else}
              <button class="btn sm" onclick={() => checkUpdates({ manual: true })} disabled={upd.checking}>
                <span class="spin-wrap" class:spin={upd.checking}><Icon name="refresh" size={14} /></span> {t('update.check')}
              </button>
            {/if}
          </SettingRow>
          {#if upd.phase === 'downloading'}
            <div class="upd-progress">
              <ProgressBar value={upd.total ? (upd.downloaded / upd.total) * 100 : 0} active height={6} />
              <span>{f.bytes(upd.downloaded)} / {f.bytes(upd.total)}{#if upd.speed > 0}{' · '}{f.speed(upd.speed)}{/if}</span>
            </div>
          {/if}
          <SettingRow label={t('update.auto')} description={t('update.autoDesc')} checked={s.updateCheck !== false} onchange={set('updateCheck')} />
          {#if s.updateSkipped}
            <SettingRow label={t('update.skipped', { version: s.updateSkipped })} description={t('update.skippedDesc')}>
              <button class="btn sm" onclick={() => saveSettings({ updateSkipped: '' })}>{t('update.unskip')}</button>
            </SettingRow>
          {/if}
        </div>
      </section>
      {/if}

      {#if show('about')}
      <section id="sec-about" class:hit={secHit('about')}>
        <h2><Icon name="info" size={18} /> {t('settings.section.about')}</h2>
        <div class="card about">
          <div class="brand">
            <div class="logo-big"><Icon name="download" size={26} stroke={2.4} /></div>
            <div>
              <div class="app-name grad-text">Nebula</div>
              <div class="muted">{t('settings.version', { version: ui.info?.version })}</div>
            </div>
          </div>
          {#if isServer}
            <p class="muted data-path"><Icon name="folder" size={13} /> <code class="selectable">{ui.info?.dataDir}</code></p>
          {/if}
          {#if isDesktopApp}
            <div class="links">
              <button class="btn sm" onclick={() => invoke('open_path', { path: ui.info.dataDir })}><Icon name="folder" size={14} /> {t('settings.dataDir')}</button>
              <button class="btn sm" onclick={() => invoke('open_path', { path: ui.info.logDir })}><Icon name="logs" size={14} /> {t('settings.logsDir')}</button>
            </div>
          {/if}
          <p class="legal">{@html th('settings.legal')}</p>
        </div>
      </section>
      {/if}
    </div>
  </div>
{/if}

<style>
  :global(.select.lang) { min-width: 220px; }
  .wrap { display: flex; height: 100%; max-width: 1140px; margin: 0 auto; }
  .toc { width: 230px; flex-shrink: 0; padding: 26px 10px 20px 30px; }
  h1 { margin: 0; font-family: var(--font-display); font-size: 26px; font-weight: 700; }
  .saved { display: flex; align-items: center; gap: 6px; margin: 6px 0 18px; font-size: 12px; color: var(--muted); }
  .toc nav { display: flex; flex-direction: column; gap: 2px; }
  .toc nav button {
    display: flex; align-items: center; gap: 10px;
    height: 36px; padding: 0 12px;
    border: none; border-radius: 9px;
    background: transparent; color: var(--text-2);
    cursor: pointer; text-align: left; font-weight: 550;
    transition: background 0.2s, color 0.2s;
  }
  .toc nav button:hover { background: var(--panel-2); color: var(--text); }
  .toc nav button.sel { background: rgba(var(--accent-rgb), 0.14); color: var(--text); }
  .toc nav button.sel :global(svg) { color: var(--accent-2); }
  .content { flex: 1; overflow-y: auto; padding: 26px 30px 120px 20px; scroll-behavior: smooth; }
  section { margin-bottom: 30px; scroll-margin-top: 20px; transition: opacity 0.25s; }
  section.dim { opacity: 0.5; }
  h2 { display: flex; align-items: center; gap: 10px; font-size: 15px; font-weight: 650; margin: 0 0 12px; color: var(--text); }
  h2 :global(svg) { color: var(--accent-2); }
  .group { overflow: hidden; }
  .path { width: 280px; height: 32px; font-size: 12.5px; }
  .small { width: 90px; height: 32px; text-align: right; }
  .unit { font-size: 12.5px; color: var(--muted); width: 32px; }
  .patterns { padding: 14px 18px 16px; border-top: 1px solid var(--border); transition: opacity 0.2s; }
  .patterns.disabled { opacity: 0.5; pointer-events: none; }
  .p-head { display: flex; justify-content: space-between; gap: 14px; }
  .p-title { font-weight: 560; }
  .p-desc { font-size: 12.5px; color: var(--muted); margin-top: 2px; }
  code { font-family: var(--mono); font-size: 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; align-items: center; }
  .pchip { display: inline-flex; align-items: center; gap: 4px; height: 28px; padding: 0 4px 0 10px; border-radius: 8px; background: var(--panel-3); border: 1px solid var(--border-2); }
  .pchip button { display: grid; place-items: center; width: 20px; height: 20px; border: none; border-radius: 5px; background: none; color: var(--muted); cursor: pointer; }
  .pchip button:hover { background: rgba(248, 113, 113, 0.18); color: var(--err); }
  .p-add .input { height: 28px; width: 160px; font-size: 12.5px; }
  .seg { display: flex; padding: 3px; gap: 3px; border-radius: 10px; background: var(--panel-2); border: 1px solid var(--border); }
  .seg button {
    display: inline-flex; align-items: center; gap: 6px;
    height: 28px; padding: 0 12px; border: none; border-radius: 7px;
    background: transparent; color: var(--text-2); font-size: 12.5px; font-weight: 600; cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .seg button:hover:not(:disabled) { color: var(--text); }
  .seg button.sel { background: rgba(var(--accent-rgb), 0.2); color: var(--text); box-shadow: inset 0 0 0 1px rgba(var(--accent-rgb), 0.45); }
  .seg button:disabled { opacity: 0.4; cursor: not-allowed; }
  .note { display: flex; align-items: center; gap: 8px; padding: 0 18px 12px 64px; margin-top: -4px; font-size: 12.5px; color: var(--muted); }
  .note.err { color: var(--err); }
  .sab { margin: 4px 18px 14px 64px; padding: 12px 14px; border-radius: 12px; background: var(--panel-2); border: 1px solid var(--border); font-size: 12.5px; color: var(--text-2); display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
  .sab-title { display: flex; align-items: center; gap: 8px; font-weight: 650; color: var(--text); font-size: 13px; }
  .sab p { margin: 0; color: var(--muted); line-height: 1.5; }
  .sab ol { margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 6px; line-height: 1.5; }
  .sab-grid { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; align-items: center; margin-top: 6px; }
  .sab-grid > span:nth-child(odd) { color: var(--muted); font-size: 12px; }
  .sab-grid code { font-family: var(--mono); font-size: 12px; padding: 2px 7px; border-radius: 6px; background: var(--panel-3); justify-self: start; overflow-wrap: anywhere; }
  .sab-warn { display: flex; align-items: center; gap: 8px; color: var(--warn); flex-wrap: wrap; }
  .sab-hint { display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; }
  .urls { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; min-width: 0; }
  .url { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; }
  .url code { padding: 4px 8px; border-radius: 7px; background: var(--panel-3); border: 1px solid var(--border-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: all; }
  .key { width: 230px; height: 32px; font-family: var(--mono); font-size: 12px; }
  .icon { width: 32px; padding: 0; justify-content: center; }
  .swatches { display: flex; gap: 10px; }
  .swatch {
    width: 30px; height: 30px; border-radius: 50%;
    border: 2px solid transparent;
    background: linear-gradient(135deg, var(--a), var(--b));
    cursor: pointer; display: grid; place-items: center; color: #fff;
    transition: transform 0.2s var(--ease), box-shadow 0.2s;
  }
  .swatch:hover { transform: scale(1.12); }
  .swatch.sel { box-shadow: 0 0 0 2px var(--bg), 0 0 0 4px var(--a); }
  .about { padding: 20px; }
  .brand { display: flex; align-items: center; gap: 16px; }
  .logo-big { width: 56px; height: 56px; border-radius: 16px; display: grid; place-items: center; background: var(--grad-diag); color: #fff; box-shadow: 0 10px 30px -10px rgba(var(--accent-rgb), 0.9); }
  .app-name { font-family: var(--font-display); font-size: 22px; font-weight: 750; }
  .links { display: flex; gap: 8px; margin-top: 16px; }
  .data-path { display: flex; align-items: center; gap: 6px; margin: 12px 0 0; font-size: 12.5px; overflow-wrap: anywhere; }
  .pw { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 12px 18px; border-top: 1px solid var(--border); }
  .pw-title { width: 100%; font-size: 13.5px; font-weight: 600; }
  .pw .input { flex: 1 1 180px; min-width: 0; }
  .upd-progress { display: flex; align-items: center; gap: 12px; padding: 0 18px 14px 64px; margin-top: -4px; font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .upd-progress span { flex-shrink: 0; }
  .spin-wrap { display: inline-flex; }
  .legal { font-size: 12px; color: var(--muted); line-height: 1.6; margin: 16px 0 0; }

  .find {
    display: flex; align-items: center; gap: 8px;
    height: 36px; margin: 0 0 14px; padding: 0 12px;
    border-radius: 10px; border: 1px solid var(--border); background: var(--panel);
    color: var(--muted);
    transition: border-color 0.15s;
  }
  .find:focus-within { border-color: rgba(var(--accent-rgb), 0.5); }
  .find input { flex: 1; min-width: 0; border: none; outline: none; background: none; color: var(--text); font: inherit; font-size: 13px; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 50px 20px; color: var(--muted); text-align: center; }
  /* Buscando: solo las filas que coinciden y las secciones que tienen alguna (o cuyo título coincide). */
  .searching :global(section.hit .srow.miss) { display: flex; }
  .searching :global(section:not(.hit):not(:has(.srow:not(.miss)))) { display: none; }
  .searching :global(section:not(.hit) .patterns),
  .searching :global(section:not(.hit) .note),
  .searching :global(section:not(.hit) .upd-progress) { display: none; }
  .searching section.dim { opacity: 1; }
  .embedded .content { padding: 0; overflow: visible; }
  .embedded section { margin-bottom: 18px; }

  .single .content { padding-top: 6px; overflow: visible; }
  .single section { margin-bottom: 0; }
  .single section > h2 { display: none; }
  @media (max-width: 720px) {
    .wrap { flex-direction: column; }
    .toc { width: auto; padding: 16px 14px 0; }
    .find { margin-bottom: 10px; }
    .toc nav { flex-direction: row; overflow-x: auto; gap: 6px; padding-bottom: 10px; scrollbar-width: none; }
    .toc nav button { flex-shrink: 0; height: 34px; padding: 0 12px; border-radius: 999px; background: var(--panel); border: 1px solid var(--border); white-space: nowrap; }
    .toc nav button.sel { border-color: rgba(var(--accent-rgb), 0.4); }
    h1 { font-size: 23px; }
    .saved { margin: 4px 0 12px; }
    .content { padding: 8px 14px 40px; }
    section { margin-bottom: 24px; }
    :global(.select.lang) { min-width: 0; width: 100%; }
    .path { width: 100%; }
    .note { padding: 0 14px 12px; }
    .sab { margin: 4px 14px 14px; }
    .urls { align-items: stretch; width: 100%; }
    .url code { flex: 1; }
    .key { flex: 1; width: auto; min-width: 0; }
    .upd-progress { padding: 0 14px 14px; flex-direction: column; align-items: stretch; gap: 6px; }
    .patterns { padding: 14px; }
    .p-head { flex-direction: column; }
    .links { flex-wrap: wrap; }
  }
</style>
