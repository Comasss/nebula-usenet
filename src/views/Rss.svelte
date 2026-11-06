<script>
  // Descarga automática por RSS: reglas, feeds y registro de capturas. En el escritorio es una
  // pestaña de Indexadores; en Android, una página de Ajustes.
  import { t, tn } from '../lib/i18n.svelte.js';
  import { invoke, listen } from '../lib/transport.js';
    import { onMount } from 'svelte';
  import { fly, fade } from 'svelte/transition';
  import Icon from '../components/Icon.svelte';
  import Modal from '../components/Modal.svelte';
  import Switch from '../components/Switch.svelte';
  import Select from '../components/Select.svelte';
  import { ui, saveSettings, ask, toast, d } from '../lib/state.svelte.js';
  import { isAndroid, pickFolder } from '../lib/platform.js';
  import { viaOptions } from '../lib/debrid.js';
  import { categoryList, categoryDir, findCategory } from '../lib/categories.js';
  import { bytes, ago, dateTime } from '../lib/format.js';

  const MIN_INTERVAL = 5;
  const DEFAULT_INTERVAL = 15;

  let status = $state({ feeds: [], log: [], busy: false, paused: false });
  let checking = $state(false);
  let editing = $state(null);
  let isNew = $state(false);
  let feedEdit = $state(null);
  let preview = $state(null);
  let previewBusy = $state(false);
  let previewFilter = $state('matched');
  let tick = $state(0);

  const s = $derived(ui.settings);
  const rules = $derived(s?.rssRules ?? []);
  const feeds = $derived(status.feeds);
  const paused = $derived(ui.queue.paused);
  const ruleCats = $derived([{ value: '', label: t('cats.byNewznab') }, { value: '*', label: t('cats.default') }, ...categoryList(s).map((c) => ({ value: c.name, label: c.name }))]);
  const vias = $derived([{ value: '', label: t('rss.viaDefault') }, ...viaOptions(s).map((o) => ({ value: o.value, label: o.child ? `· ${o.label}` : o.label }))]);

  async function refresh() {
    try {
      status = await invoke('rss_status');
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    refresh();
    let un;
    listen('rss', refresh).then((f) => (un = f));
    const timer = setInterval(() => tick++, 30000);
    return () => {
      un?.();
      clearInterval(timer);
    };
  });

  // Los cambios en indexadores o reglas cambian la lista de feeds.
  $effect(() => {
    void s?.indexers;
    void s?.rssFeeds;
    void s?.rssRules;
    void s?.featureIndexers;
    refresh();
  });

  async function checkNow() {
    checking = true;
    try {
      const r = await invoke('rss_check_now');
      if (!r.feeds) toast('info', t('rss.nothingToCheck'), 3500);
      else toast(r.errors ? 'error' : 'success', t('rss.checked', { feeds: r.feeds, grabbed: r.grabbed }), 3500);
    } catch (e) {
      toast('error', String(e));
    } finally {
      checking = false;
      refresh();
    }
  }

  // ---- Reglas ----

  function blankRule() {
    return {
      id: crypto.randomUUID().replaceAll('-', ''),
      name: '',
      enabled: true,
      feeds: [],
      must: '',
      regex: false,
      exclude: '',
      minSizeMb: 0,
      maxSizeMb: 0,
      categories: '',
      via: '',
      destDir: '',
      category: '',
      createdAt: Math.floor(Date.now() / 1000),
      onlyNew: true,
    };
  }

  function addRule() {
    editing = blankRule();
    isNew = true;
    preview = null;
  }

  function editRule(r) {
    editing = structuredClone($state.snapshot(r));
    editing.category ??= '';
    isNew = false;
    preview = null;
  }

  function cleanRule() {
    const r = $state.snapshot(editing);
    r.name = r.name.trim() || r.must.trim().slice(0, 40) || t('rss.untitled');
    r.minSizeMb = Math.max(0, Math.round(Number(r.minSizeMb) || 0));
    r.maxSizeMb = Math.max(0, Math.round(Number(r.maxSizeMb) || 0));
    return r;
  }

  async function saveRule() {
    const r = cleanRule();
    if (r.regex) {
      try {
        new RegExp(r.must);
      } catch {
        toast('error', t('rss.badRegex'));
        return;
      }
    }
    if (r.maxSizeMb && r.minSizeMb > r.maxSizeMb) {
      toast('error', t('rss.badSize'));
      return;
    }
    const all = $state.snapshot(rules);
    const i = all.findIndex((x) => x.id === r.id);
    if (i >= 0) all[i] = r;
    else all.push(r);
    if (await saveSettings({ rssRules: all })) {
      toast('success', isNew ? t('rss.ruleAdded') : t('rss.ruleUpdated'), 2500);
      editing = null;
    }
  }

  async function removeRule(r) {
    const res = await ask({ title: t('rss.removeRuleTitle'), message: t('rss.removeRuleMessage', { name: r.name }), confirmLabel: t('common.remove'), danger: true, icon: 'trash' });
    if (!res.ok) return;
    await saveSettings({ rssRules: $state.snapshot(rules).filter((x) => x.id !== r.id) });
  }

  async function toggleRule(r, v) {
    await saveSettings({ rssRules: $state.snapshot(rules).map((x) => (x.id === r.id ? { ...x, enabled: v } : x)) });
  }

  function toggleFeedInRule(id) {
    const i = editing.feeds.indexOf(id);
    if (i >= 0) editing.feeds.splice(i, 1);
    else editing.feeds.push(id);
  }

  async function pickDest() {
    const dir = await pickFolder({ defaultPath: editing.destDir || s.downloadDir, title: t('rss.dest') });
    if (dir) editing.destDir = dir;
  }

  async function runPreview() {
    previewBusy = true;
    try {
      preview = await invoke('rss_preview', { rule: cleanRule() });
      previewFilter = preview.items.some((i) => i.matched) ? 'matched' : 'all';
      refresh();
    } catch (e) {
      preview = null;
      toast('error', String(e));
    } finally {
      previewBusy = false;
    }
  }

  const previewItems = $derived(!preview ? [] : previewFilter === 'matched' ? preview.items.filter((i) => i.matched) : preview.items);
  const previewMatches = $derived(preview ? preview.items.filter((i) => i.matched).length : 0);

  function why(c) {
    return t(`rss.why.${c.key}.${c.ok ? 'ok' : 'no'}`, { detail: c.detail });
  }

  function ruleSummary(r) {
    const parts = [];
    if (r.must.trim()) parts.push(r.regex ? `/${r.must.trim()}/` : r.must.trim());
    if (r.exclude.trim()) parts.push(`− ${r.exclude.trim()}`);
    if (r.minSizeMb || r.maxSizeMb) parts.push(`${r.minSizeMb || 0}–${r.maxSizeMb || '∞'} MB`);
    if (r.categories.trim()) parts.push(r.categories.trim());
    return parts.join(' · ') || t('rss.matchesAll');
  }

  function ruleFeeds(r) {
    if (!r.feeds.length) return t('rss.allFeeds');
    const names = r.feeds.map((id) => feeds.find((f) => f.id === id)?.name).filter(Boolean);
    return names.length ? names.join(', ') : t('rss.noFeedLeft');
  }

  function viaName(v) {
    return vias.find((o) => o.value === v)?.label.replace(/^· /, '') ?? v;
  }

  // ---- Feeds ----

  function feedPatch(f, patch) {
    const all = $state.snapshot(s.rssFeeds ?? []);
    const i = all.findIndex((x) => x.id === f.id);
    const base = i >= 0 ? all[i] : { id: f.id, name: '', url: '', indexer: f.indexer, intervalMin: f.intervalMin, enabled: f.enabled };
    const next = { ...base, ...patch };
    if (i >= 0) all[i] = next;
    else all.push(next);
    return saveSettings({ rssFeeds: all });
  }

  function setInterval_(f, e) {
    let v = Math.round(Number(e.target.value) || DEFAULT_INTERVAL);
    if (v < MIN_INTERVAL) {
      v = MIN_INTERVAL;
      toast('info', t('rss.minInterval', { n: MIN_INTERVAL }), 3000);
    }
    e.target.value = v;
    feedPatch(f, { intervalMin: v });
  }

  function addFeed() {
    feedEdit = { id: crypto.randomUUID().replaceAll('-', ''), name: '', url: '', indexer: '', intervalMin: DEFAULT_INTERVAL, enabled: true, isNew: true };
  }

  function editFeed(f) {
    feedEdit = { ...(s.rssFeeds.find((x) => x.id === f.id) ?? f), isNew: false };
  }

  async function saveFeed() {
    const { isNew: fresh, ...f } = $state.snapshot(feedEdit);
    f.url = f.url.trim();
    if (!/^https?:\/\//i.test(f.url)) {
      toast('error', t('indexers.urlInvalid'));
      return;
    }
    f.intervalMin = Math.max(MIN_INTERVAL, Math.round(Number(f.intervalMin) || DEFAULT_INTERVAL));
    if (!f.name.trim()) f.name = host(f.url);
    const all = $state.snapshot(s.rssFeeds ?? []);
    const i = all.findIndex((x) => x.id === f.id);
    if (i >= 0) all[i] = f;
    else all.push(f);
    if (await saveSettings({ rssFeeds: all })) {
      toast('success', fresh ? t('rss.feedAdded') : t('rss.feedUpdated'), 2500);
      feedEdit = null;
    }
  }

  async function removeFeed(f) {
    const res = await ask({ title: t('rss.removeFeedTitle'), message: t('rss.removeFeedMessage', { name: f.name }), confirmLabel: t('common.remove'), danger: true, icon: 'trash' });
    if (!res.ok) return;
    await saveSettings({
      rssFeeds: $state.snapshot(s.rssFeeds).filter((x) => x.id !== f.id),
      rssRules: $state.snapshot(rules).map((r) => ({ ...r, feeds: r.feeds.filter((id) => id !== f.id) })),
    });
  }

  function host(url) {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  }

  function feedState(f) {
    void tick;
    if (!f.enabled) return t('rss.feedOff');
    if (!f.used) return t('rss.feedUnused');
    if (!f.lastCheck) return t('rss.feedPending');
    return t('rss.feedChecked', { ago: ago(f.lastCheck), n: f.items });
  }

  async function clearLog() {
    await invoke('rss_clear_log');
  }
</script>

<div class="view rss">
  <header class="head">
    <div>
      <h1>RSS</h1>
      <p class="subtitle">{t('rss.subtitle')}</p>
    </div>
    <div class="head-actions">
      <button class="btn" onclick={checkNow} disabled={checking || status.busy || paused || !rules.length}>
        <Icon name={checking || status.busy ? 'loader' : 'refresh'} size={16} class={checking || status.busy ? 'spin' : ''} /> {t('rss.checkNow')}
      </button>
      <button class="btn primary" onclick={addRule}><Icon name="plus" size={16} /> {t('rss.newRule')}</button>
    </div>
  </header>

  {#if isAndroid}
    <div class="alert warn note"><Icon name="smartphone" size={16} /><span>{t('rss.androidNote')}</span></div>
  {/if}
  {#if paused && rules.length}
    <div class="alert info note"><Icon name="pause" size={16} /><span>{t('rss.pausedNote')}</span></div>
  {/if}
  {#if !s.featureIndexers && !(s.rssFeeds ?? []).some((f) => !f.indexer)}
    <div class="alert info note"><Icon name="info" size={16} /><span>{t('rss.noFeedsNote')}</span></div>
  {/if}

  <h2>{t('rss.rules')}</h2>
  <div class="list">
    {#each rules as r (r.id)}
      <article class="item card" class:off={!r.enabled} in:fly={{ y: 10, duration: d(250) }}>
        <div class="row">
          <div class="main">
            <div class="name">{r.name}{#if !r.onlyNew}<span class="chip muted">{t('rss.includesOld')}</span>{/if}</div>
            <div class="sub selectable">{ruleSummary(r)}</div>
            <div class="sub">
              <Icon name="rss" size={12} /> {ruleFeeds(r)}
              {#if r.via}· <Icon name="download" size={12} /> {viaName(r.via)}{/if}
              {#if r.category}· <Icon name="tag" size={12} /> {r.category}{/if}
              {#if r.destDir}· <Icon name="folder" size={12} /> <span class="truncate">{r.destDir}</span>{/if}
            </div>
          </div>
          <div class="actions">
            <button class="icon-btn" title={t('common.edit')} onclick={() => editRule(r)}><Icon name="edit" size={16} /></button>
            <button class="icon-btn danger" title={t('common.remove')} onclick={() => removeRule(r)}><Icon name="trash" size={16} /></button>
            <Switch checked={r.enabled} onchange={(v) => toggleRule(r, v)} label={t('common.enabled')} />
          </div>
        </div>
      </article>
    {:else}
      <div class="empty card">
        <Icon name="rss" size={32} stroke={1.6} />
        <h3>{t('rss.emptyTitle')}</h3>
        <p>{t('rss.emptyText')}</p>
        <button class="btn primary" onclick={addRule}><Icon name="plus" size={16} /> {t('rss.newRule')}</button>
      </div>
    {/each}
  </div>

  <div class="h2row">
    <h2>{t('rss.feeds')}</h2>
    <button class="btn sm" onclick={addFeed}><Icon name="plus" size={14} /> {t('rss.addFeed')}</button>
  </div>
  <p class="hint">{t('rss.feedsHint')}</p>
  <div class="list">
    {#each feeds as f (f.id)}
      <article class="item card feed" class:off={!f.enabled || !f.used}>
        <div class="row">
          <div class="ficon" class:ix={f.indexer}><Icon name={f.indexer ? 'layers' : 'rss'} size={16} /></div>
          <div class="main">
            <div class="name">{f.name}<span class="chip muted">{f.indexer ? t('rss.indexerFeed') : host(f.url)}</span></div>
            <div class="sub">{feedState(f)}</div>
            {#if f.lastError && f.enabled}<div class="sub err selectable"><Icon name="x-circle" size={12} /> {f.lastError}</div>{/if}
          </div>
          <div class="actions">
            <label class="every" title={t('rss.intervalDesc', { n: MIN_INTERVAL })}>
              <input class="input small" type="number" min={MIN_INTERVAL} max="1440" value={f.intervalMin} onchange={(e) => setInterval_(f, e)} />
              <span>min</span>
            </label>
            {#if !f.indexer}
              <button class="icon-btn" title={t('common.edit')} onclick={() => editFeed(f)}><Icon name="edit" size={16} /></button>
              <button class="icon-btn danger" title={t('common.remove')} onclick={() => removeFeed(f)}><Icon name="trash" size={16} /></button>
            {/if}
            <Switch checked={f.enabled} onchange={(v) => feedPatch(f, { enabled: v })} label={t('common.enabled')} />
          </div>
        </div>
      </article>
    {:else}
      <div class="empty card small"><p>{t('rss.noFeeds')}</p></div>
    {/each}
  </div>

  <div class="h2row">
    <h2>{t('rss.log')}</h2>
    {#if status.log.length}<button class="btn sm ghost" onclick={clearLog}><Icon name="trash" size={14} /> {t('common.clear')}</button>{/if}
  </div>
  {#if status.log.length}
    <div class="card log">
      <div class="lrow lhead">
        <span>{t('rss.colDate')}</span><span>{t('rss.colRule')}</span><span>{t('rss.colRelease')}</span><span>{t('rss.colIndexer')}</span>
      </div>
      {#each status.log as c, i (i)}
        <div class="lrow" class:bad={!c.ok}>
          <span class="when" title={dateTime(c.at)}>{dateTime(c.at)}</span>
          <span class="truncate">{c.rule}</span>
          <span class="rel selectable">
            {#if c.ok}<Icon name="check-circle" size={13} />{:else}<Icon name="x-circle" size={13} />{/if}
            <span class="truncate" title={c.title}>{c.title}</span>
            {#if c.error}<small class="err">{c.error}</small>{/if}
          </span>
          <span class="truncate">{c.indexer}</span>
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty card small"><p>{t('rss.logEmpty')}</p></div>
  {/if}
</div>

{#if editing}
  <Modal title={isNew ? t('rss.newRule') : t('rss.editRule')} subtitle={t('rss.ruleSubtitle')} icon="rss" width={760} onclose={() => (editing = null)}>
    <div class="form">
      <div class="field">
        <label for="r-name">{t('common.name')}</label>
        <input id="r-name" class="input" bind:value={editing.name} placeholder={t('rss.namePlaceholder')} />
      </div>

      <div class="field">
        <span class="label">{t('rss.ruleFeeds')}</span>
        <div class="chips">
          <button type="button" class="pick" class:sel={!editing.feeds.length} onclick={() => (editing.feeds = [])}>{t('rss.allFeeds')}</button>
          {#each feeds as f (f.id)}
            <button type="button" class="pick" class:sel={editing.feeds.includes(f.id)} onclick={() => toggleFeedInRule(f.id)}>
              <Icon name={f.indexer ? 'layers' : 'rss'} size={12} /> {f.name}
            </button>
          {/each}
        </div>
      </div>

      <div class="field">
        <div class="lab-row">
          <label for="r-must">{editing.regex ? t('rss.mustRegex') : t('rss.must')}</label>
          <label class="inline"><Switch checked={editing.regex} onchange={(v) => (editing.regex = v)} label="Regex" /> Regex</label>
        </div>
        <input id="r-must" class="input mono" bind:value={editing.must} spellcheck="false" placeholder={editing.regex ? '^Some\\.Show\\.S\\d+E\\d+.*1080p' : t('rss.mustPlaceholder')} />
        <small class="help">{editing.regex ? t('rss.mustRegexHelp') : t('rss.mustHelp')}</small>
      </div>

      <div class="field">
        <label for="r-ex">{t('rss.exclude')}</label>
        <input id="r-ex" class="input mono" bind:value={editing.exclude} spellcheck="false" placeholder={t('rss.excludePlaceholder')} />
      </div>

      <div class="grid3">
        <div class="field">
          <label for="r-min">{t('rss.minSize')}</label>
          <input id="r-min" class="input" type="number" min="0" bind:value={editing.minSizeMb} />
        </div>
        <div class="field">
          <label for="r-max">{t('rss.maxSize')}</label>
          <input id="r-max" class="input" type="number" min="0" bind:value={editing.maxSizeMb} />
        </div>
        <div class="field">
          <label for="r-cat">{t('rss.categories')}</label>
          <input id="r-cat" class="input" bind:value={editing.categories} placeholder="5000, 2040, TV" />
        </div>
      </div>

      {#if categoryList(s).length}
        <div class="field">
          <span class="label">{t('cats.category')}</span>
          <Select bind:value={editing.category} options={ruleCats} label={t('cats.category')} />
          <small class="help">{t('cats.ruleHelp')}</small>
        </div>
      {/if}

      <div class="grid2">
        <div class="field">
          <span class="label">{t('rss.via')}</span>
          <Select bind:value={editing.via} options={vias} label={t('rss.via')} />
        </div>
        <div class="field">
          <label for="r-dest">{t('rss.dest')}</label>
          <div class="dest">
            <input id="r-dest" class="input" bind:value={editing.destDir} placeholder={categoryDir(s, findCategory(s, editing.category))} />
            <button class="btn sm" onclick={pickDest}>{t('common.browse')}</button>
          </div>
          <small class="help">{t('rss.destHelp')}</small>
        </div>
      </div>

      <label class="inline only-new">
        <Switch checked={editing.onlyNew} onchange={(v) => (editing.onlyNew = v)} label={t('rss.onlyNew')} />
        <span><strong>{t('rss.onlyNew')}</strong><small>{t('rss.onlyNewDesc', { date: dateTime(editing.createdAt) })}</small></span>
      </label>

      <div class="pv">
        <div class="pv-head">
          <button class="btn" onclick={runPreview} disabled={previewBusy}>
            <Icon name={previewBusy ? 'loader' : 'eye'} size={16} class={previewBusy ? 'spin' : ''} /> {t('rss.preview')}
          </button>
          {#if preview}
            <span class="pv-count">{tn('rss.previewCount', previewMatches, { total: preview.items.length })}</span>
            <div class="seg">
              <button class:sel={previewFilter === 'matched'} onclick={() => (previewFilter = 'matched')}>{t('rss.onlyMatches')}</button>
              <button class:sel={previewFilter === 'all'} onclick={() => (previewFilter = 'all')}>{t('rss.allItems')}</button>
            </div>
          {:else}
            <span class="pv-count">{t('rss.previewHint')}</span>
          {/if}
        </div>
        {#if preview}
          {#each preview.errors as [name, err]}
            <div class="alert err"><Icon name="x-circle" size={14} /><span>{name}: {err}</span></div>
          {/each}
          <div class="pv-list" in:fade={{ duration: d(150) }}>
            {#each previewItems as it, i (i)}
              <div class="pv-item" class:hit={it.matched}>
                <div class="pv-title">
                  <Icon name={it.matched ? 'check-circle' : 'x-circle'} size={14} />
                  <span class="selectable">{it.title}</span>
                </div>
                <div class="pv-meta">
                  {it.indexer || it.feed} · {it.size ? bytes(it.size) : '?'} · {dateTime(it.date)}{it.category ? ` · ${it.category}` : ''}
                </div>
                <div class="why">
                  {#each it.checks as c}
                    <span class="w" class:ok={c.ok}>{why(c)}</span>
                  {/each}
                  {#if it.matched && it.seen}<span class="w warn">{t('rss.why.seen')}</span>{/if}
                  {#if it.matched && it.duplicate}<span class="w warn">{t('rss.why.duplicate')}</span>{/if}
                  {#if !it.checks.length}<span class="w ok">{t('rss.why.any')}</span>{/if}
                </div>
              </div>
            {:else}
              <p class="hint">{t('rss.previewEmpty')}</p>
            {/each}
          </div>
        {/if}
      </div>
    </div>
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (editing = null)}>{t('common.cancel')}</button>
      <button class="btn primary" onclick={saveRule}><Icon name="check" size={16} /> {t('common.save')}</button>
    {/snippet}
  </Modal>
{/if}

{#if feedEdit}
  <Modal title={feedEdit.isNew ? t('rss.addFeed') : t('rss.editFeed')} icon="rss" width={520} onclose={() => (feedEdit = null)}>
    <div class="form">
      <div class="field">
        <label for="f-url">URL</label>
        <input id="f-url" class="input" bind:value={feedEdit.url} placeholder="https://…/rss" spellcheck="false" />
      </div>
      <div class="grid2">
        <div class="field">
          <label for="f-name">{t('common.name')}</label>
          <input id="f-name" class="input" bind:value={feedEdit.name} placeholder={host(feedEdit.url)} />
        </div>
        <div class="field">
          <label for="f-int">{t('rss.interval')}</label>
          <input id="f-int" class="input" type="number" min={MIN_INTERVAL} bind:value={feedEdit.intervalMin} />
        </div>
      </div>
      <small class="help">{t('rss.intervalDesc', { n: MIN_INTERVAL })}</small>
    </div>
    {#snippet footer()}
      <button class="btn ghost" onclick={() => (feedEdit = null)}>{t('common.cancel')}</button>
      <button class="btn primary" onclick={saveFeed}><Icon name="check" size={16} /> {t('common.save')}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .view { padding: 26px 30px 40px; max-width: 1100px; margin: 0 auto; }
  .head { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; }
  .head-actions { display: flex; gap: 8px; flex-wrap: wrap; }
  h1 { margin: 0; font-family: var(--font-display); font-size: 26px; font-weight: 700; }
  .subtitle { margin: 4px 0 0; color: var(--text-2); }
  h2 { margin: 26px 0 10px; font-size: 13px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.07em; color: var(--muted); }
  .h2row { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .h2row h2 { margin-bottom: 10px; }
  .note { margin-top: 14px; }
  .hint { margin: -4px 0 10px; color: var(--muted); font-size: 12.5px; }
  .list { display: flex; flex-direction: column; gap: 10px; }
  .item { padding: 14px 16px; transition: opacity 0.3s; }
  .item.off { opacity: 0.6; }
  .row { display: flex; align-items: center; gap: 14px; }
  .main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .name { display: flex; align-items: center; gap: 8px; font-weight: 620; font-size: 15px; flex-wrap: wrap; }
  .sub { display: flex; align-items: center; gap: 5px; color: var(--text-2); font-size: 12.5px; min-width: 0; flex-wrap: wrap; }
  .sub.err, .err { color: #f08080; }
  .actions { display: flex; align-items: center; gap: 2px; }
  .actions :global(.switch) { margin-left: 8px; }
  .ficon { width: 34px; height: 34px; border-radius: 10px; display: grid; place-items: center; background: var(--panel-3); color: #f2a65a; flex-shrink: 0; }
  .ficon.ix { color: var(--accent-2); }
  .every { display: flex; align-items: center; gap: 4px; margin-right: 6px; color: var(--muted); font-size: 12px; }
  .every .input { width: 64px; }
  .empty { padding: 30px 20px; text-align: center; color: var(--text-2); display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .empty.small { padding: 16px; }
  .empty h3 { margin: 6px 0 0; color: var(--text); font-family: var(--font-display); }
  .empty p { margin: 0 0 8px; }
  .empty.small p { margin: 0; }

  .log { padding: 4px 0; overflow: hidden; }
  .lrow { display: grid; grid-template-columns: 150px 1fr 2.4fr 1fr; gap: 12px; padding: 8px 16px; font-size: 13px; align-items: center; }
  .lrow + .lrow { border-top: 1px solid var(--border); }
  .lhead { font-size: 11.5px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  .lrow .when { color: var(--text-2); font-variant-numeric: tabular-nums; }
  .rel { display: flex; align-items: center; gap: 6px; min-width: 0; flex-wrap: wrap; }
  .rel :global(svg) { color: #5fd38d; flex-shrink: 0; }
  .lrow.bad .rel :global(svg) { color: #f08080; }
  .rel small { width: 100%; font-size: 11.5px; }

  .form { display: flex; flex-direction: column; gap: 14px; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; }
  .grid3 { display: grid; grid-template-columns: 1fr 1fr 1.4fr; gap: 14px; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .pick { display: inline-flex; align-items: center; gap: 5px; padding: 6px 11px; border-radius: 999px; border: 1px solid var(--border); background: var(--panel-2); color: var(--text-2); font-size: 12.5px; cursor: pointer; }
  .pick.sel { border-color: rgba(var(--accent-rgb), 0.6); background: rgba(var(--accent-rgb), 0.16); color: var(--text); }
  .lab-row { display: flex; align-items: center; justify-content: space-between; }
  .inline { display: flex; align-items: center; gap: 8px; font-size: 12.5px; color: var(--text-2); cursor: pointer; }
  .only-new { align-items: flex-start; }
  .only-new span { display: flex; flex-direction: column; gap: 2px; }
  .only-new strong { color: var(--text); font-size: 13.5px; font-weight: 600; }
  .help { color: var(--muted); font-size: 12px; }
  .mono { font-family: var(--font-mono, ui-monospace, monospace); }
  .dest { display: flex; gap: 8px; }
  .dest .input { flex: 1; min-width: 0; }

  .pv { border-top: 1px solid var(--border); padding-top: 14px; display: flex; flex-direction: column; gap: 10px; }
  .pv-head { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .pv-count { color: var(--text-2); font-size: 12.5px; flex: 1; }
  .seg { display: flex; border: 1px solid var(--border); border-radius: 9px; overflow: hidden; }
  .seg button { border: none; background: none; color: var(--text-2); padding: 5px 10px; font-size: 12px; cursor: pointer; }
  .seg button.sel { background: rgba(var(--accent-rgb), 0.18); color: var(--text); }
  .pv-list { display: flex; flex-direction: column; gap: 6px; max-height: 340px; overflow-y: auto; }
  .pv-item { padding: 9px 11px; border-radius: 10px; background: var(--panel-2); border: 1px solid var(--border); }
  .pv-item.hit { border-color: rgba(95, 211, 141, 0.4); }
  .pv-title { display: flex; align-items: flex-start; gap: 7px; font-size: 13px; font-weight: 560; word-break: break-word; }
  .pv-title :global(svg) { color: #f08080; flex-shrink: 0; margin-top: 2px; }
  .pv-item.hit .pv-title :global(svg) { color: #5fd38d; }
  .pv-meta { color: var(--muted); font-size: 11.5px; margin: 3px 0 6px 21px; }
  .why { display: flex; flex-wrap: wrap; gap: 5px; margin-left: 21px; }
  .w { font-size: 11.5px; padding: 2px 8px; border-radius: 999px; background: rgba(240, 128, 128, 0.14); color: #f4a3a3; }
  .w.ok { background: rgba(95, 211, 141, 0.13); color: #8fe0b0; }
  .w.warn { background: rgba(242, 166, 90, 0.15); color: #f5bf86; }

  @media (max-width: 720px) {
    .view { padding: 16px 14px 28px; }
    .head { flex-direction: column; align-items: stretch; gap: 12px; }
    .head-actions .btn { flex: 1; justify-content: center; }
    .row { flex-wrap: wrap; gap: 10px; }
    .actions { width: 100%; justify-content: flex-end; padding-top: 8px; border-top: 1px solid var(--border); }
    .grid2, .grid3 { grid-template-columns: 1fr; }
    .lhead { display: none; }
    .lrow { grid-template-columns: 1fr; gap: 2px; padding: 10px 14px; }
    .lrow .when { font-size: 11.5px; }
  }
</style>
