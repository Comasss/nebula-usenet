<script>
  import { t, th } from '../lib/i18n.svelte.js';
  import { pickFolder, isAndroid, isDesktopApp, isServer } from '../lib/platform.js';
  import { fly, fade, scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Icon from './Icon.svelte';
  import ServerForm from './ServerForm.svelte';
  import IndexerForm from './IndexerForm.svelte';
  import SettingRow from './SettingRow.svelte';
  import logo from '../assets/logo.svg';
  import { ui, saveSettings, toast, d } from '../lib/state.svelte.js';
  import { blankIndexer } from '../lib/search.js';
  import { scanCode, showReceive } from '../lib/transfer.svelte.js';

  const s0 = $state.snapshot(ui.settings);
  const firstTime = !s0.firstRunDone;
  let dir = $state(1);
  let saving = $state(false);

  // La primera vez vienen activadas las dos: descargas y búsqueda.
  let features = $state({
    nntp: s0.featuresChosen ? s0.featureNntp : true,
    indexers: s0.featuresChosen ? s0.featureIndexers : firstTime || s0.featureIndexers,
  });
  let server = $state(
    s0.servers[0]
      ? structuredClone(s0.servers[0])
      : {
          id: crypto.randomUUID().replaceAll('-', ''),
          name: '', host: '', port: 563, ssl: true, verifyCert: true, username: '', password: '', passwordEnc: '',
          connections: 20, priority: 0, enabled: true, retentionDays: 0,
        }
  );
  let indexer = $state(s0.indexers[0] ? structuredClone(s0.indexers[0]) : blankIndexer());
  let prefs = $state({
    downloadDir: s0.downloadDir,
    nzbSaveDir: s0.nzbSaveDir,
    askDestination: s0.askDestination,
    par2Enabled: s0.par2Enabled,
    extractEnabled: s0.extractEnabled,
    clipboardMonitor: s0.clipboardMonitor,
    associateNzb: s0.associateNzb || firstTime,
    closeToTray: s0.closeToTray,
    searchDedup: s0.searchDedup,
    searchQuickAdd: s0.searchQuickAdd,
  });

    const steps = $derived(
    [firstTime && 'welcome', 'features', features.indexers && 'indexers', features.nntp && 'server', 'prefs', 'done'].filter(Boolean)
  );
  let current = $state(firstTime ? 'welcome' : 'features');
  const idx = $derived(steps.indexOf(current));

  const mode = $derived(
    features.nntp && features.indexers ? 'both' : features.indexers ? 'indexers' : features.nntp ? 'nntp' : 'none'
  );

  let skipServer = $state(false);
  let skipIndexer = $state(false);

  function go(step) {
    dir = steps.indexOf(step) > idx ? 1 : -1;
    current = step;
  }
  const next = () => go(steps[idx + 1]);
  const back = () => go(steps[idx - 1]);

  function nextFromFeatures() {
    if (mode === 'none') {
      toast('warn', t('wizard.enableOne'));
      return;
    }
    next();
  }

  function nextFromServer() {
    if (!server.host.trim()) {
      toast('error', t('wizard.serverRequired'));
      return;
    }
    if (!server.username.trim()) toast('warn', t('wizard.noUsername'), 4000);
    skipServer = false;
    next();
  }

  function nextFromIndexer() {
    if (!/^https?:\/\//i.test(indexer.url.trim())) {
      toast('error', t('wizard.indexerRequired'));
      return;
    }
    if (!indexer.apiKey.trim()) toast('warn', t('wizard.noApiKey'), 4000);
    skipIndexer = false;
    next();
  }

  async function browse(key) {
    const p = await pickFolder({ defaultPath: prefs[key] });
    if (p) prefs[key] = p;
  }

  async function finish() {
    saving = true;
    const servers = $state.snapshot(ui.settings.servers);
    if (features.nntp && !skipServer && server.host.trim()) {
      const item = $state.snapshot(server);
      item.port = Number(item.port) || (item.ssl ? 563 : 119);
      const i = servers.findIndex((x) => x.id === item.id);
      if (i >= 0) servers[i] = item;
      else servers.push(item);
    }
    const indexers = $state.snapshot(ui.settings.indexers);
    if (features.indexers && !skipIndexer && indexer.url.trim()) {
      const item = $state.snapshot(indexer);
      item.url = item.url.trim().replace(/\/+$/, '');
      if (!item.name.trim()) item.name = item.url.replace(/^https?:\/\/(api\.)?/i, '').split(/[/.]/)[0];
      const i = indexers.findIndex((x) => x.id === item.id);
      if (i >= 0) indexers[i] = item;
      else indexers.push(item);
    }
    const ok = await saveSettings({
      ...$state.snapshot(prefs),
      servers,
      indexers,
      featureNntp: features.nntp,
      featureIndexers: features.indexers,
      featuresChosen: true,
      firstRunDone: true,
    });
    saving = false;
    if (ok) {
      ui.setupOpen = false;
      ui.view = features.indexers ? 'search' : 'downloads';
    }
  }
</script>

<div class="overlay" transition:fade={{ duration: d(220) }}>
  <div class="wizard" in:scale={{ start: 0.95, duration: d(320), easing: cubicOut }}>
    <div class="glow"></div>
    <div class="steps">
      {#each steps as st, i (st)}
        <div class="stp" class:done={i < idx} class:cur={i === idx}>
          <span class="num">{#if i < idx}<Icon name="check" size={12} stroke={3} />{:else}{i + 1}{/if}</span>
          <span class="lbl">{t(`wizard.step.${st}`)}</span>
        </div>
        {#if i < steps.length - 1}<div class="line" class:done={i < idx}></div>{/if}
      {/each}
    </div>

    <div class="stage">
      {#key current}
        <div class="page" in:fly={{ x: 40 * dir, duration: d(360), easing: cubicOut }} out:fly={{ x: -40 * dir, duration: d(200) }}>
          {#if current === 'welcome'}
            <div class="welcome">
              <img class="hero" src={logo} alt="" />
              <h1>{@html th('wizard.welcomeTitle')}</h1>
              <p>{t('wizard.welcomeText')}</p>
              <div class="features-list">
                <div><Icon name="search" size={16} /> {t('wizard.featSearch')}</div>
                <div><Icon name="zap" size={16} /> {t('wizard.featConns')}</div>
                <div><Icon name="shield" size={16} /> {t('wizard.featPar2')}</div>
                <div><Icon name="archive" size={16} /> {t('wizard.featExtract')}</div>
              </div>
              <button type="button" class="btn ghost from-device" onclick={isAndroid ? scanCode : showReceive}>
                <Icon name="qr" size={16} /> {t('transfer.wizardLink')}
              </button>
            </div>
          {:else if current === 'features'}
            <h2>{firstTime ? t('wizard.featuresTitle') : t('wizard.featuresNew')}</h2>
            <p class="lead">{t('wizard.featuresLead')}</p>
            <div class="fcards">
              <button type="button" class="fcard" class:on={features.indexers} onclick={() => (features.indexers = !features.indexers)}>
                <div class="f-top">
                  <div class="f-icon"><Icon name="search" size={22} /></div>
                  <span class="f-check">{#if features.indexers}<span in:scale={{ duration: d(200) }}><Icon name="check" size={14} stroke={3} /></span>{/if}</span>
                </div>
                <div class="f-title">{t('nav.indexers')}</div>
                <div class="f-desc">{t('wizard.indexersDesc')}</div>
              </button>
              <button type="button" class="fcard" class:on={features.nntp} onclick={() => (features.nntp = !features.nntp)}>
                <div class="f-top">
                  <div class="f-icon"><Icon name="server" size={22} /></div>
                  <span class="f-check">{#if features.nntp}<span in:scale={{ duration: d(200) }}><Icon name="check" size={14} stroke={3} /></span>{/if}</span>
                </div>
                <div class="f-title">{t('wizard.nntpTitle')}</div>
                <div class="f-desc">{t('wizard.nntpDesc')}</div>
              </button>
            </div>
            <div class="mode alert {mode === 'none' ? 'warn' : 'info'}">
              <Icon name={mode === 'none' ? 'alert-triangle' : 'info'} size={16} />
              {#key mode}<span in:fade={{ duration: d(200) }}>{t(`wizard.mode.${mode}`)}</span>{/key}
            </div>
          {:else if current === 'indexers'}
            <h2>{t('wizard.indexerTitle')}</h2>
            <p class="lead">{t('wizard.indexerLead')}</p>
            <IndexerForm bind:indexer compact />
          {:else if current === 'server'}
            <h2>{t('wizard.serverTitle')}</h2>
            <p class="lead">{t('wizard.serverLead')}</p>
            <ServerForm bind:server compact />
            <div class="alert info wz-debrid">
              <Icon name="cloud" size={15} />
              <span>{@html th('wizard.debridHint')}</span>
            </div>
          {:else if current === 'prefs'}
            <h2>{t('wizard.step.prefs')}</h2>
            <p class="lead">{t('wizard.prefsLead')}</p>
            {#if features.nntp}
              <div class="field">
                <label for="wz-dir">{t('settings.downloadDir')}</label>
                <div class="dirrow">
                  <input id="wz-dir" class="input" bind:value={prefs.downloadDir} />
                  <button class="btn" onclick={() => browse('downloadDir')}>{t('common.browse')}…</button>
                </div>
              </div>
            {:else}
              <div class="field">
                <label for="wz-nzb">{t('wizard.nzbDir')}</label>
                <div class="dirrow">
                  <input id="wz-nzb" class="input" bind:value={prefs.nzbSaveDir} />
                  <button class="btn" onclick={() => browse('nzbSaveDir')}>{t('common.browse')}…</button>
                </div>
                <span class="hint">{t('wizard.nzbDirHint')}</span>
              </div>
            {/if}
            <div class="card prefs">
              {#if features.indexers}
                <SettingRow label={t('wizard.dedup')} description={t('wizard.dedupDesc')} checked={prefs.searchDedup} onchange={(v) => (prefs.searchDedup = v)} />
              {/if}
              {#if features.indexers && features.nntp}
                <SettingRow label={t('wizard.quickAdd')} description={t('wizard.quickAddDesc')} checked={prefs.searchQuickAdd} onchange={(v) => (prefs.searchQuickAdd = v)} />
              {/if}
              {#if features.nntp}
                <SettingRow label={t('wizard.askDestination')} checked={prefs.askDestination} onchange={(v) => (prefs.askDestination = v)} />
                <SettingRow label={t('settings.par2')} checked={prefs.par2Enabled} onchange={(v) => (prefs.par2Enabled = v)} />
                <SettingRow label={t('settings.extract')} checked={prefs.extractEnabled} onchange={(v) => (prefs.extractEnabled = v)} />
                {#if isDesktopApp}
                  <SettingRow label={t('settings.associate')} checked={prefs.associateNzb} onchange={(v) => (prefs.associateNzb = v)} />
                {/if}
              {/if}
              {#if !isServer}
                <SettingRow label={t('wizard.clipboard')} checked={prefs.clipboardMonitor} onchange={(v) => (prefs.clipboardMonitor = v)} />
              {/if}
              {#if isDesktopApp}
                <SettingRow label={t('wizard.closeToTray')} checked={prefs.closeToTray} onchange={(v) => (prefs.closeToTray = v)} />
              {/if}
            </div>
          {:else}
            <div class="welcome">
              <div class="done-badge"><Icon name="check" size={40} stroke={2.6} /></div>
              <h1>{t('wizard.doneTitle')}</h1>
              <p>
                {#if features.indexers}{@html th('wizard.doneSearch', { target: skipIndexer || !indexer.url.trim() ? t('wizard.yourIndexers') : indexer.name || t('wizard.yourIndexer') })}{/if}
                {#if features.nntp && server.host.trim() && !skipServer}{@html th('wizard.doneServer', { host: server.host, n: server.connections })}{/if}
                {#if !features.indexers}{t('wizard.doneNntp')}{/if}
              </p>
            </div>
          {/if}
        </div>
      {/key}
    </div>

    <footer>
      {#if current === 'server'}
        <button class="btn ghost" onclick={() => { skipServer = true; next(); }}>{t('wizard.skip')}</button>
      {:else if current === 'indexers'}
        <button class="btn ghost" onclick={() => { skipIndexer = true; next(); }}>{t('wizard.skip')}</button>
      {/if}
      <div class="spacer"></div>
      {#if idx > 0}
        <button class="btn" onclick={back}><Icon name="chevron-left" size={16} /> {t('wizard.back')}</button>
      {/if}
      {#if current === 'welcome'}
        <button class="btn primary lg" onclick={next}>{t('wizard.start')} <Icon name="chevron-right" size={16} /></button>
      {:else if current === 'features'}
        <button class="btn primary" onclick={nextFromFeatures} disabled={mode === 'none'}>{t('wizard.next')} <Icon name="chevron-right" size={16} /></button>
      {:else if current === 'indexers'}
        <button class="btn primary" onclick={nextFromIndexer}>{t('wizard.next')} <Icon name="chevron-right" size={16} /></button>
      {:else if current === 'server'}
        <button class="btn primary" onclick={nextFromServer}>{t('wizard.next')} <Icon name="chevron-right" size={16} /></button>
      {:else if current === 'prefs'}
        <button class="btn primary" onclick={next}>{t('wizard.next')} <Icon name="chevron-right" size={16} /></button>
      {:else}
        <button class="btn primary lg" onclick={finish} disabled={saving}><Icon name="rocket" size={16} /> {t('wizard.start')}</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: var(--titlebar-h) 0 0 0; z-index: 200; display: grid; place-items: center; background: rgba(5, 6, 10, 0.75); backdrop-filter: blur(10px); padding: 24px; }
  .wizard { position: relative; width: min(780px, 100%); max-height: calc(100vh - 90px); display: flex; flex-direction: column; background: var(--panel); border: 1px solid var(--border-2); border-radius: 22px; box-shadow: var(--shadow); overflow: hidden; }
  .glow { position: absolute; inset: -200px -100px auto auto; width: 500px; height: 400px; background: radial-gradient(circle, rgba(var(--accent-rgb), 0.25), transparent 65%); pointer-events: none; }
  .wz-debrid { margin-top: 14px; }
  .from-device { margin-top: 18px; color: var(--accent-2); }
  .steps { position: relative; display: flex; align-items: center; gap: 10px; padding: 22px 28px 0; }
  .stp { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--muted); font-weight: 600; white-space: nowrap; }
  .num { width: 24px; height: 24px; border-radius: 50%; display: grid; place-items: center; font-size: 11.5px; font-weight: 700; background: var(--panel-3); color: var(--text-2); transition: all 0.3s var(--ease); }
  .stp.cur { color: var(--text); }
  .stp.cur .num { background: var(--grad-diag); color: #fff; box-shadow: 0 0 0 4px rgba(var(--accent-rgb), 0.2); }
  .stp.done .num { background: rgba(52, 211, 153, 0.18); color: var(--ok); }
  .line { flex: 1; min-width: 12px; height: 2px; border-radius: 2px; background: var(--panel-3); position: relative; overflow: hidden; }
  .line::after { content: ''; position: absolute; inset: 0; background: var(--grad); transform: scaleX(0); transform-origin: left; transition: transform 0.5s var(--ease); }
  .line.done::after { transform: scaleX(1); }
  .stage { position: relative; display: grid; grid-template-columns: minmax(0, 1fr); overflow-y: auto; overflow-x: hidden; min-height: 420px; }
  .page { grid-area: 1 / 1; min-width: 0; padding: 26px 28px 20px; }
  .welcome { text-align: center; padding: 16px 20px 0; }
  .hero { width: 110px; height: 110px; animation: float 4s ease-in-out infinite; filter: drop-shadow(0 18px 40px rgba(var(--accent-rgb), 0.5)); }
  @keyframes float { 50% { transform: translateY(-8px) rotate(-3deg); } }
  .welcome h1 { font-family: var(--font-display); font-size: 28px; margin: 18px 0 8px; }
  .welcome p { color: var(--text-2); max-width: 500px; margin: 0 auto; line-height: 1.55; }
  .features-list { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; max-width: 540px; margin: 26px auto 0; text-align: left; }
  .features-list div { display: flex; align-items: center; gap: 10px; padding: 11px 14px; border-radius: 12px; background: var(--panel-2); border: 1px solid var(--border); font-size: 13px; color: var(--text-2); }
  .features-list :global(svg) { color: var(--accent-2); }
  h2 { font-family: var(--font-display); font-size: 21px; margin: 0; }
  .lead { color: var(--text-2); margin: 6px 0 20px; }
  .fcards { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; }
  .fcard {
    text-align: left; padding: 18px; border-radius: 16px;
    border: 1.5px solid var(--border-2); background: var(--panel-2);
    cursor: pointer; display: flex; flex-direction: column; gap: 6px;
    transition: border-color 0.25s, background 0.25s, transform 0.2s var(--ease), box-shadow 0.3s;
  }
  .fcard:hover { transform: translateY(-2px); border-color: #3a4156; }
  .fcard.on { border-color: rgba(var(--accent-rgb), 0.75); background: linear-gradient(160deg, rgba(var(--accent-rgb), 0.16), transparent 70%), var(--panel-2); box-shadow: 0 14px 40px -20px rgba(var(--accent-rgb), 0.9); }
  .f-top { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 6px; }
  .f-icon { width: 44px; height: 44px; border-radius: 12px; display: grid; place-items: center; background: var(--panel-3); color: var(--text-2); transition: all 0.3s; }
  .fcard.on .f-icon { background: var(--grad-diag); color: #fff; }
  .f-check { width: 24px; height: 24px; border-radius: 50%; border: 1.5px solid var(--border-2); display: grid; place-items: center; color: #fff; transition: all 0.2s; }
  .fcard.on .f-check { background: var(--grad-diag); border-color: transparent; }
  .f-title { font-weight: 650; font-size: 15px; }
  .f-desc { font-size: 12.5px; color: var(--text-2); line-height: 1.5; }
  .mode { margin-top: 16px; }
  .dirrow { display: flex; gap: 8px; }
  .prefs { margin-top: 16px; overflow: hidden; }
  .done-badge { width: 86px; height: 86px; margin: 20px auto 0; border-radius: 50%; display: grid; place-items: center; background: linear-gradient(135deg, #10b981, #34d399); color: #fff; box-shadow: 0 16px 50px -12px rgba(52, 211, 153, 0.8); animation: pop-in 0.6s var(--ease); }
  @keyframes pop-in { 0% { transform: scale(0.3); opacity: 0; } 60% { transform: scale(1.08); } }
  footer { display: flex; align-items: center; gap: 10px; padding: 16px 28px; border-top: 1px solid var(--border); background: rgba(0, 0, 0, 0.15); }

  /* Móvil: a pantalla completa y en una columna. */
  :global(html.mobile) .overlay { padding: 0; place-items: stretch; background: var(--panel); }
  :global(html.mobile) .wizard { width: 100%; max-height: none; height: 100%; border: none; border-radius: 0; box-shadow: none; }
  :global(html.mobile) .steps { padding: 18px 18px 0; gap: 6px; }
  :global(html.mobile) .stp .lbl { display: none; }
  :global(html.mobile) .stp.cur .lbl { display: inline; }
  :global(html.mobile) .stage { min-height: 0; flex: 1; }
  :global(html.mobile) .page { padding: 22px 18px 18px; }
  :global(html.mobile) .welcome { padding: 8px 4px 0; }
  :global(html.mobile) .from-device { white-space: normal; height: auto; min-height: 42px; padding: 8px 12px; text-align: left; }
  :global(html.mobile) .welcome h1 { font-size: 24px; }
  :global(html.mobile) .features-list, :global(html.mobile) .fcards { grid-template-columns: 1fr; }
  :global(html.mobile) .dirrow { flex-direction: column; }
  :global(html.mobile) footer { padding: 12px 16px; flex-wrap: wrap; }
</style>
