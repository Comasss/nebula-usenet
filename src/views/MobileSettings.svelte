<script>
  // Configuración en el móvil: una portada con secciones (fuentes de descarga, preferencias y
  // sistema) y cada una se abre como una página con su botón Atrás.
  import { t, tn } from '../lib/i18n.svelte.js';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Icon from '../components/Icon.svelte';
  import logo from '../assets/logo.svg';
  import { ui, d } from '../lib/state.svelte.js';
  import { haptic } from '../lib/feedback.js';
  import { usableDebrid, nntpServers } from '../lib/debrid.js';
  import { upd } from '../lib/updater.svelte.js';
  import Settings from './Settings.svelte';
  import { fold } from '../lib/format.js';
  import { isServer } from '../lib/platform.js';
  import Indexers from './Indexers.svelte';
  import Servers from './Servers.svelte';
  import Debrid from './Debrid.svelte';
  import Logs from './Logs.svelte';
  import Rss from './Rss.svelte';

  const s = $derived(ui.settings);
  const PAGES = { indexers: Indexers, servers: Servers, debrid: Debrid, logs: Logs, rss: Rss };

  const indexerCount = $derived((s?.indexers ?? []).filter((i) => i.enabled).length);
  const serverCount = $derived(nntpServers(s).length);
  const debridCount = $derived(usableDebrid(s).length);
  const rssCount = $derived((s?.rssRules ?? []).filter((r) => r.enabled).length);
  const errorLogs = $derived(ui.logs.filter((l) => l.level === 'error').length);

  const sources = $derived(
    [
      s?.featureIndexers && { id: 'indexers', icon: 'layers', hue: 265, label: t('nav.indexers'), sub: indexerCount ? tn('msettings.active', indexerCount) : t('msettings.noneYet') },
      s?.featureNntp !== false && { id: 'servers', icon: 'server', hue: 190, label: t('nav.servers'), sub: serverCount ? tn('msettings.active', serverCount) : t('msettings.noneYet') },
      s?.featureNntp !== false && { id: 'debrid', icon: 'cloud', hue: 215, label: t('nav.debrid'), sub: debridCount ? tn('msettings.active', debridCount) : t('msettings.noneYet') },
      s?.featureNntp !== false && { id: 'rss', icon: 'rss', hue: 30, label: t('nav.rss'), sub: rssCount ? tn('rss.activeRules', rssCount) : t('msettings.desc.rss') },
    ].filter(Boolean)
  );

  const SECTION_ROWS = [
    { id: 'features', icon: 'sparkles', hue: 290 },
    { id: 'language', icon: 'globe', hue: 160 },
    { id: 'debrid', icon: 'download', hue: 230, nntp: true },
    { id: 'search', icon: 'search', hue: 265, indexers: true },
    { id: 'folders', icon: 'folder', hue: 40 },
    { id: 'categories', icon: 'tag', hue: 70 },
    { id: 'post', icon: 'package', hue: 20 },
    { id: 'links', icon: 'clipboard', hue: 330 },
    { id: 'windows', icon: 'smartphone', hue: 200 },
    { id: 'server', icon: 'user', hue: 200 },
    { id: 'finish', icon: 'bell', hue: 10 },
    { id: 'network', icon: 'wifi', hue: 140 },
    { id: 'schedule', icon: 'clock', hue: 180, nntp: true },
    { id: 'look', icon: 'palette', hue: 300 },
    { id: 'remote', icon: 'monitor', hue: 250 },
    { id: 'api', icon: 'plug', hue: 100 },
    { id: 'transfer', icon: 'qr', hue: 50 },
    { id: 'import', icon: 'download', hue: 120 },
  ];
  // En el navegador (nebula-server) no hay portapapeles, ventana ni actualizador; sí la cuenta.
  const NOT_HERE = isServer ? ['links', 'windows', 'updates'] : ['server'];
  const prefs = $derived(SECTION_ROWS.filter((r) => !NOT_HERE.includes(r.id) && (!r.nntp || s?.featureNntp !== false) && (!r.indexers || s?.featureIndexers)));

  function sectionTitle(id) {
    if (id === 'debrid') return t('settings.downloadMethod');
    if (id === 'search') return t('settings.searchTitle');
    if (id === 'windows') return t('mobile.appTitle');
    if (id === 'api') return t('api.title');
    if (id === 'remote') return isServer ? t('settings.section.remote') : t('remote.mobileTitle');
    if (id === 'server') return t('web.section');
    return t(`settings.section.${id}`);
  }

  const page = $derived(ui.settingsPage);
  const title = $derived(!page ? '' : page.startsWith('sec:') ? sectionTitle(page.slice(4)) : t(`nav.${page}`));

  // Buscador: muestra las páginas y los ajustes de todas las secciones que coinciden.
  let query = $state('');
  const pageHits = $derived.by(() => {
    const q = fold(query);
    if (!q) return [];
    const logs = { id: 'logs', icon: 'logs', hue: 0, label: t('nav.logs'), sub: t('msettings.desc.logs') };
    return [...sources, logs].filter((r) => fold(`${r.label} ${r.sub}`).includes(q));
  });

  let dir = $state(1);
  function open(id) {
    haptic('tap');
    dir = 1;
    ui.settingsPage = id;
  }
  function back() {
    haptic('tap');
    dir = -1;
    ui.settingsPage = null;
  }
  // El botón Atrás de Android también vuelve a la portada: la animación va hacia atrás.
  $effect(() => {
    if (!ui.settingsPage) dir = -1;
  });
</script>

{#snippet row(r, onclick, badge = 0)}
  <button class="s-row" {onclick} style="--h:{r.hue}">
    <span class="tile"><Icon name={r.icon} size={17} stroke={2.2} /></span>
    <span class="r-text">
      <strong>{r.label}</strong>
      {#if r.sub}<small class="truncate">{r.sub}</small>{/if}
    </span>
    {#if badge}<span class="r-badge">{badge}</span>{/if}
    <Icon name="chevron-right" size={16} />
  </button>
{/snippet}

{#if s}
  {#key page}
    <div class="ms" in:fly={{ x: 36 * dir, duration: d(300), easing: cubicOut, opacity: 0 }}>
      {#if !page}
        <header class="hero">
          <h1>{t('nav.settings')}</h1>
          <p><Icon name="check" size={13} /> {t('settings.autosave')}</p>
        </header>

        <label class="find">
          <Icon name="search" size={16} />
          <input type="search" placeholder={t('settings.searchPlaceholder')} bind:value={query} />
          {#if query}
            <button onclick={() => (query = '')} aria-label={t('common.clear')}><Icon name="x" size={14} /></button>
          {/if}
        </label>

        {#if fold(query)}
          {#if pageHits.length}
            <div class="card group hits">
              {#each pageHits as r (r.id)}{@render row(r, () => open(r.id))}{/each}
            </div>
          {/if}
          <div class="results"><Settings query={query} /></div>
        {:else}

        <div class="about card">
          <img src={logo} alt="" width="52" height="52" />
          <div class="a-text">
            <strong class="grad-text">Nebula</strong>
            <span>{t('msettings.version', { version: ui.info?.version })}</span>
          </div>
        </div>

        {#if sources.length}
          <h3>{t('msettings.sources')}</h3>
          <div class="card group">
            {#each sources as r (r.id)}{@render row(r, () => open(r.id))}{/each}
          </div>
        {/if}

        <h3>{t('msettings.prefs')}</h3>
        <div class="card group">
          {#each prefs as r (r.id)}
            {@render row({ ...r, label: sectionTitle(r.id), sub: t(`msettings.desc.${r.id}`) }, () => open(`sec:${r.id}`))}
          {/each}
        </div>

        <h3>{t('msettings.system')}</h3>
        <div class="card group">
          {@render row({ id: 'logs', icon: 'logs', hue: 0, label: t('nav.logs'), sub: t('msettings.desc.logs') }, () => open('logs'), errorLogs)}
          {#if !isServer}{@render row({ id: 'updates', icon: 'refresh', hue: 170, label: t('settings.section.updates'), sub: upd.info?.available ? t('update.availableLabel', { version: upd.info.version }) : t('msettings.desc.updates') }, () => open('sec:updates'), upd.info?.available ? 1 : 0)}{/if}
          {@render row({ id: 'about', icon: 'info', hue: 250, label: t('settings.section.about'), sub: t('msettings.desc.about') }, () => open('sec:about'))}
        </div>
        {/if}
      {:else}
        <header class="bar">
          <button class="back" onclick={back} aria-label={t('common.back')}><Icon name="chevron-left" size={20} stroke={2.4} /></button>
          <h2 class="truncate">{title}</h2>
        </header>
        <div class="subpage" class:sec={page.startsWith('sec:')}>
          {#if page.startsWith('sec:')}
            <Settings only={page.slice(4)} />
          {:else}
            {@const P = PAGES[page]}
            {#if P}<P />{/if}
          {/if}
        </div>
      {/if}
    </div>
  {/key}
{/if}

<style>
  .ms { padding: 10px 14px 24px; }
  .hero { padding: 8px 4px 14px; }
  .hero h1 { margin: 0; font-family: var(--font-display); font-size: 30px; font-weight: 750; letter-spacing: -0.01em; }
  .hero p { display: flex; align-items: center; gap: 6px; margin: 4px 0 0; font-size: 12.5px; color: var(--muted); }
  .about {
    display: flex; align-items: center; gap: 14px;
    padding: 16px;
    background:
      radial-gradient(140% 120% at 0% 0%, rgba(var(--accent-rgb), 0.2), transparent 60%),
      linear-gradient(180deg, rgba(255, 255, 255, 0.03), transparent), var(--panel);
    border-color: rgba(var(--accent-rgb), 0.22);
  }
  .about img { border-radius: 14px; filter: drop-shadow(0 8px 18px rgba(var(--accent-rgb), 0.45)); }
  .a-text { display: flex; flex-direction: column; flex: 1; min-width: 0; }
  .a-text strong { font-family: var(--font-display); font-size: 21px; font-weight: 750; }
  .a-text span { font-size: 12.5px; color: var(--muted); }
  h3 { margin: 22px 6px 8px; font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); }
  .group { overflow: hidden; padding: 4px 0; }
  .find {
    display: flex; align-items: center; gap: 10px;
    height: 44px; margin: 0 0 14px; padding: 0 14px;
    border-radius: 14px; border: 1px solid var(--border); background: var(--panel);
    color: var(--muted);
  }
  .find:focus-within { border-color: rgba(var(--accent-rgb), 0.5); }
  .find input { flex: 1; min-width: 0; border: none; outline: none; background: none; color: var(--text); font: inherit; font-size: 15px; }
  .find input::-webkit-search-cancel-button { display: none; }
  .find button { display: grid; place-items: center; width: 26px; height: 26px; border: none; border-radius: 50%; background: var(--panel-3); color: var(--text-2); }
  .hits { margin-bottom: 16px; }
  .s-row {
    display: flex; align-items: center; gap: 13px;
    width: 100%; min-height: 60px; padding: 8px 14px;
    border: none; background: transparent; color: var(--text-2);
    text-align: left; cursor: pointer;
    transition: background 0.15s;
  }
  .s-row + .s-row { border-top: 1px solid rgba(255, 255, 255, 0.04); }
  .s-row:active { background: rgba(255, 255, 255, 0.04); }
  .s-row:active .tile { transform: scale(0.9); }
  .tile {
    width: 36px; height: 36px; border-radius: 11px; flex-shrink: 0;
    display: grid; place-items: center;
    color: hsl(var(--h) 90% 78%);
    background: linear-gradient(135deg, hsl(var(--h) 70% 45% / 0.35), hsl(var(--h) 70% 35% / 0.18));
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.12), inset 0 0 0 1px hsl(var(--h) 70% 60% / 0.25);
    transition: transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .r-text { display: flex; flex-direction: column; flex: 1; min-width: 0; }
  .r-text strong { color: var(--text); font-size: 15px; font-weight: 600; }
  .r-text small { font-size: 12.5px; color: var(--muted); }
  .r-badge { min-width: 22px; height: 22px; padding: 0 7px; border-radius: 999px; display: grid; place-items: center; font-size: 11.5px; font-weight: 700; color: #fff; background: #dc5050; }
  .s-row > :global(svg:last-child) { color: var(--muted); flex-shrink: 0; }

  .bar {
    position: sticky; top: 0; z-index: 5;
    display: flex; align-items: center; gap: 10px;
    margin: -10px -14px 0; padding: 10px 14px;
    background: linear-gradient(180deg, var(--bg-2) 60%, transparent);
  }
  .back {
    width: 40px; height: 40px; border-radius: 50%; flex-shrink: 0;
    display: grid; place-items: center;
    border: 1px solid var(--glass-border);
    background: var(--glass-bg);
    box-shadow: var(--glass-shine);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    color: var(--text); cursor: pointer;
    transition: transform 0.2s var(--ease);
  }
  .back:active { transform: scale(0.88); }
  .bar h2 { margin: 0; font-family: var(--font-display); font-size: 21px; font-weight: 700; }
  .subpage { margin: 0 -14px; }
  .subpage.sec { margin: 0; }
  /* Las vistas incrustadas no repiten su título (ya está en la barra). */
  .subpage :global(.view > .head h1) { display: none; }
  .subpage :global(.view) { padding-top: 4px; }
  .subpage :global(.wrap.single) { height: auto; }
  .subpage :global(.wrap.single .content) { padding: 4px 0 0; }
</style>
