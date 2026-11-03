<script module>
  // El periodo elegido se mantiene al cambiar de vista.
  const sel = $state({ range: 'd30', from: '', to: '', tab: null });
</script>

<script>
  import { t, tn, num, locale } from '../lib/i18n.svelte.js';
  import { call } from '../lib/remote.svelte.js';
  import { fly, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Icon from '../components/Icon.svelte';
  import IndexerStats from '../components/IndexerStats.svelte';
  import DateRangePicker from '../components/DateRangePicker.svelte';
  import { ui, d } from '../lib/state.svelte.js';
  import { debridLabel } from '../lib/debrid.js';
  import * as f from '../lib/format.js';

  const DAY = 86400;
  const RANGES = [
    ['d7', 7],
    ['d30', 30],
    ['d90', 90],
    ['year', 365],
    ['all', null],
    ['custom', null],
  ];

  let records = $state([]);
  let events = $state([]);
  let loaded = $state(false);

  const nntp = $derived(ui.settings?.featureNntp !== false);
  const idx = $derived(!!ui.settings?.featureIndexers);
  const tab = $derived(sel.tab === 'indexers' && idx ? 'indexers' : sel.tab === 'downloads' && nntp ? 'downloads' : nntp ? 'downloads' : 'indexers');

  async function load() {
    const [r, e] = await Promise.all([call('get_stats').catch(() => []), call('get_indexer_stats').catch(() => [])]);
    records = r ?? [];
    events = e ?? [];
    loaded = true;
  }

  // Se recargan cuando termina una descarga (el historial cambia a la vez) y cada poco
  // mientras la vista está abierta, para incluir las búsquedas nuevas.
  $effect(() => {
    ui.history;
    load();
  });
  $effect(() => {
    const id = setInterval(load, 15000);
    return () => clearInterval(id);
  });

  const ymd = (date) => {
    const p = (n) => String(n).padStart(2, '0');
    return `${date.getFullYear()}-${p(date.getMonth() + 1)}-${p(date.getDate())}`;
  };
  const parseDay = (s, end) => {
    const [y, m, dd] = s.split('-').map(Number);
    if (!y || !m || !dd) return null;
    return new Date(y, m - 1, dd, end ? 23 : 0, end ? 59 : 0, end ? 59 : 0).getTime() / 1000;
  };

  function pick(range) {
    if (range === 'custom' && !sel.from) {
      const now = new Date();
      sel.to = ymd(now);
      sel.from = ymd(new Date(now.getTime() - 29 * DAY * 1000));
    }
    sel.range = range;
  }

  const period = $derived.by(() => {
    const now = Date.now() / 1000;
    if (sel.range === 'custom') {
      const a = parseDay(sel.from, false);
      const b = parseDay(sel.to, true);
      if (a === null || b === null) return { from: 0, to: now };
      return a <= b ? { from: a, to: b } : { from: parseDay(sel.to, false), to: parseDay(sel.from, true) };
    }
    if (sel.range === 'all') {
      const first = tab === 'indexers' ? events.reduce((m, e) => Math.min(m, e.at), now) : records.reduce((m, r) => Math.min(m, r.at), now);
      return { from: Math.min(first, now - DAY), to: now, all: true };
    }
    const days = RANGES.find(([k]) => k === sel.range)?.[1] ?? 30;
    return { from: now - days * DAY, to: now };
  });

  const inRange = $derived(records.filter((r) => period.all || (r.at >= period.from && r.at <= period.to)));
  const hasData = $derived(tab === 'indexers' ? events.length > 0 : records.length > 0);

  const total = $derived(inRange.length);
  const okCount = $derived(inRange.filter((r) => r.ok).length);
  const failCount = $derived(total - okCount);
  const rate = (ok, n) => (n ? (ok / n) * 100 : null);
  const overall = $derived(rate(okCount, total));
  const okSize = $derived(inRange.filter((r) => r.ok).reduce((a, r) => a + r.size, 0));

  function serverName(id, fallback) {
    const s = ui.settings?.servers?.find((x) => x.id === id);
    return s ? { name: s.name || s.host, gone: false } : { name: fallback || id, gone: true };
  }

  // Por servidor NNTP: NZB en los que sirvió o le faltó al menos un artículo.
  const servers = $derived.by(() => {
    const map = new Map();
    for (const r of inRange) {
      for (const u of r.servers ?? []) {
        if (!u.ok && !u.missing) continue;
        let s = map.get(u.id);
        if (!s) map.set(u.id, (s = { id: u.id, stored: u.name, nzbs: 0, ok: 0, artOk: 0, artMissing: 0, bytes: 0 }));
        s.stored = u.name || s.stored;
        s.nzbs++;
        if (r.ok) s.ok++;
        s.artOk += u.ok;
        s.artMissing += u.missing;
        s.bytes += u.bytes;
      }
    }
    const order = (id) => ui.settings?.servers?.find((x) => x.id === id)?.priority ?? 999;
    return [...map.values()]
      .map((s) => ({ ...s, ...serverName(s.id, s.stored), rate: rate(s.ok, s.nzbs), articles: rate(s.artOk, s.artOk + s.artMissing) }))
      .sort((a, b) => order(a.id) - order(b.id) || b.nzbs - a.nzbs);
  });

  // Por servicio debrid: los NZB que se descargaron a través de él.
  const debrid = $derived.by(() => {
    const map = new Map();
    for (const r of inRange) {
      if (!r.via || r.via === 'nntp') continue;
      let s = map.get(r.via);
      if (!s) map.set(r.via, (s = { id: r.via, stored: r.viaName, nzbs: 0, ok: 0, bytes: 0 }));
      s.nzbs++;
      if (r.ok) {
        s.ok++;
        s.bytes += r.size;
      }
    }
    return [...map.values()]
      .map((s) => {
        const cfg = ui.settings?.debrid?.find((x) => x.id === s.id);
        return { ...s, name: cfg ? debridLabel(cfg) : s.stored || s.id, gone: !cfg, rate: rate(s.ok, s.nzbs) };
      })
      .sort((a, b) => b.nzbs - a.nzbs);
  });

  const nntpWithoutDetail = $derived(inRange.some((r) => (!r.via || r.via === 'nntp') && !r.servers?.length));

  const pct = (v) => (v === null ? '—' : `${num(v, { maximumFractionDigits: 1, minimumFractionDigits: v > 0 && v < 100 ? 1 : 0 })}\u00a0%`);
  const tone = (v) => (v === null ? 'muted' : v >= 90 ? 'ok' : v >= 60 ? 'warn' : 'err');
  const day = (ts) => new Date(ts * 1000).toLocaleDateString(locale(), { day: 'numeric', month: 'short', year: 'numeric' });
</script>

<div class="view">
  <header class="head">
    <div>
      <h1>{t('nav.stats')}</h1>
      <p class="subtitle">
        {#if hasData || sel.range !== 'all'}
          {t('stats.showing', { from: day(period.from), to: day(period.to) })}
        {:else}
          {t('stats.subtitle')}
        {/if}
      </p>
    </div>
  </header>

  {#if nntp && idx}
    <div class="tabs">
      {#each [['downloads', 'download', t('stats.tab.downloads')], ['indexers', 'layers', t('stats.tab.indexers')]] as [k, icon, label]}
        <button class="tab" class:sel={tab === k} onclick={() => (sel.tab = k)}>
          <Icon name={icon} size={15} /> {label}
          {#if tab === k}<span class="underline"></span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  <div class="toolbar">
    <div class="seg" role="tablist">
      {#each RANGES as [k]}
        <button class="seg-btn" class:sel={sel.range === k} role="tab" aria-selected={sel.range === k} onclick={() => pick(k)}>
          {#if k === 'custom'}<Icon name="calendar" size={14} />{/if}
          {t(`stats.range.${k}`)}
        </button>
      {/each}
    </div>
    {#if sel.range === 'custom'}
      <div in:fly={{ x: -8, duration: d(220) }}>
        <DateRangePicker bind:from={sel.from} bind:to={sel.to} />
      </div>
    {/if}
  </div>

  {#if tab === 'indexers'}
    <IndexerStats {events} {records} {period} {loaded} />
  {:else}
  <div class="kpis">
    <div class="kpi card">
      <div class="k-label"><Icon name="inbox" size={14} /> {t('stats.total')}</div>
      <div class="k-value">{num(total)}</div>
      <div class="k-foot">{t('stats.downloaded', { size: f.bytes(okSize) })}</div>
    </div>
    <div class="kpi card">
      <div class="k-label"><Icon name="check-circle" size={14} /> {t('stats.completed')}</div>
      <div class="k-value ok">{num(okCount)}</div>
      <div class="k-foot">{t('stats.ofTotal', { n: num(okCount), total: num(total) })}</div>
    </div>
    <div class="kpi card">
      <div class="k-label"><Icon name="x-circle" size={14} /> {t('stats.failed')}</div>
      <div class="k-value" class:err={failCount > 0}>{num(failCount)}</div>
      <div class="k-foot">{t('stats.ofTotal', { n: num(failCount), total: num(total) })}</div>
    </div>
    <div class="kpi card main">
      <div class="k-label"><Icon name="gauge" size={14} /> {t('stats.successRate')}</div>
      <div class="k-value grad-text">{pct(overall)}</div>
      <div class="bar"><span class={tone(overall)} style="width:{overall ?? 0}%"></span></div>
    </div>
  </div>

  <section>
    <div class="sec-head">
      <h2><Icon name="server" size={17} /> {t('stats.servers')}</h2>
      <p class="hint">{t('stats.serversHint')}</p>
    </div>
    {#if servers.length}
      <div class="table card">
        <div class="tr th">
          <span>{t('stats.col.server')}</span>
          <span class="r">{t('stats.col.nzbs')}</span>
          <span class="r">{t('stats.completed')}</span>
          <span class="r">{t('stats.failed')}</span>
          <span>{t('stats.col.rate')}</span>
          <span class="r">{t('stats.col.articles')}</span>
          <span class="r">{t('stats.col.data')}</span>
        </div>
        {#each servers as s (s.id)}
          <div class="tr" in:fly={{ y: 8, duration: d(260), easing: cubicOut }}>
            <span class="name">
              <span class="truncate" title={s.name}>{s.name}</span>
              {#if s.gone}<span class="chip muted">{t('stats.removed')}</span>{/if}
            </span>
            <span class="r num">{num(s.nzbs)}</span>
            <span class="r num ok-t">{num(s.ok)}</span>
            <span class="r num" class:err-t={s.nzbs - s.ok > 0}>{num(s.nzbs - s.ok)}</span>
            <span class="rate">
              <span class="bar"><span class={tone(s.rate)} style="width:{s.rate ?? 0}%"></span></span>
              <span class="num pct">{pct(s.rate)}</span>
            </span>
            <span class="r num" title={t('stats.articlesTip', { ok: num(s.artOk), missing: num(s.artMissing) })}>{pct(s.articles)}</span>
            <span class="r num muted">{f.bytes(s.bytes)}</span>
          </div>
        {/each}
      </div>
    {:else if loaded}
      <div class="empty card" in:fade={{ duration: d(200) }}>
        <Icon name="server" size={28} stroke={1.6} />
        <p>{total && nntpWithoutDetail ? t('stats.noServerDetail') : t('stats.noData')}</p>
      </div>
    {/if}
    {#if servers.length && nntpWithoutDetail}
      <p class="hint note"><Icon name="info" size={13} /> {t('stats.noServerDetail')}</p>
    {/if}
  </section>

  {#if debrid.length}
    <section in:fade={{ duration: d(200) }}>
      <div class="sec-head">
        <h2><Icon name="cloud" size={17} /> {t('stats.debrid')}</h2>
      </div>
      <div class="table card deb">
        <div class="tr th">
          <span>{t('stats.col.service')}</span>
          <span class="r">{t('stats.col.nzbs')}</span>
          <span class="r">{t('stats.completed')}</span>
          <span class="r">{t('stats.failed')}</span>
          <span>{t('stats.col.rate')}</span>
          <span class="r">{t('stats.col.data')}</span>
        </div>
        {#each debrid as s (s.id)}
          <div class="tr">
            <span class="name">
              <span class="truncate" title={s.name}>{s.name}</span>
              {#if s.gone}<span class="chip muted">{t('stats.removed')}</span>{/if}
            </span>
            <span class="r num">{num(s.nzbs)}</span>
            <span class="r num ok-t">{num(s.ok)}</span>
            <span class="r num" class:err-t={s.nzbs - s.ok > 0}>{num(s.nzbs - s.ok)}</span>
            <span class="rate">
              <span class="bar"><span class={tone(s.rate)} style="width:{s.rate ?? 0}%"></span></span>
              <span class="num pct">{pct(s.rate)}</span>
            </span>
            <span class="r num muted">{f.bytes(s.bytes)}</span>
          </div>
        {/each}
      </div>
    </section>
  {/if}
  {/if}
</div>

<style>
  .view { padding: 26px 30px 40px; max-width: 1100px; margin: 0 auto; }
  .head { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; }
  h1 { margin: 0; font-family: var(--font-display); font-size: 26px; font-weight: 700; }
  .subtitle { margin: 4px 0 0; color: var(--text-2); }

  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 12px 16px; margin-top: 20px; }
  .tabs { display: flex; gap: 4px; margin-top: 16px; border-bottom: 1px solid var(--border); }
  .tab { position: relative; height: 40px; padding: 0 14px; border: none; background: none; color: var(--text-2); font-weight: 560; cursor: pointer; display: flex; align-items: center; gap: 8px; }
  .tab:hover, .tab.sel { color: var(--text); }
  .tab.sel :global(svg) { color: var(--accent-2); }
  .underline { position: absolute; left: 10px; right: 10px; bottom: -1px; height: 2px; border-radius: 2px; background: var(--grad); animation: grow 0.3s var(--ease); }
  @keyframes grow { from { transform: scaleX(0.3); opacity: 0; } }
  .seg { display: inline-flex; padding: 3px; gap: 2px; border-radius: 11px; background: var(--panel); border: 1px solid var(--border); }
  .seg-btn {
    height: 32px; padding: 0 12px;
    display: inline-flex; align-items: center; gap: 6px;
    border: 1px solid transparent; border-radius: 8px;
    background: transparent; color: var(--text-2);
    font-size: 13px; font-weight: 580; cursor: pointer; white-space: nowrap;
    transition: background 0.2s var(--ease), color 0.2s, border-color 0.2s;
  }
  .seg-btn:hover:not(.sel) { background: var(--panel-3); color: var(--text); }
  .seg-btn.sel {
    color: var(--text);
    background: linear-gradient(90deg, rgba(var(--accent-rgb), 0.22), rgba(var(--accent-rgb), 0.08));
    border-color: rgba(var(--accent-rgb), 0.35);
  }
  .seg-btn.sel :global(svg) { color: var(--accent-2); }

  .kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin-top: 18px; }
  .kpi { padding: 16px 18px; display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .kpi.main { border-color: rgba(var(--accent-rgb), 0.3); background: linear-gradient(160deg, rgba(var(--accent-rgb), 0.12), transparent 70%), var(--panel); }
  .k-label { display: flex; align-items: center; gap: 7px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.07em; color: var(--muted); font-weight: 650; }
  .k-value { font-family: var(--font-display); font-size: 30px; font-weight: 700; font-variant-numeric: tabular-nums; line-height: 1.15; }
  .k-value.ok { color: var(--ok); }
  .k-value.err { color: var(--err); }
  .k-foot { font-size: 12.5px; color: var(--text-2); }
  .kpi .bar { margin-top: 6px; }

  .bar { position: relative; height: 6px; border-radius: 99px; background: var(--panel-3); overflow: hidden; flex: 1; }
  .bar > span { position: absolute; left: 0; top: 0; bottom: 0; border-radius: 99px; transition: width 0.6s var(--ease); }
  .bar > .ok { background: linear-gradient(90deg, #10b981, var(--ok)); }
  .bar > .warn { background: linear-gradient(90deg, #f59e0b, var(--warn)); }
  .bar > .err { background: linear-gradient(90deg, #ef4444, var(--err)); }
  .bar > .muted { background: transparent; }

  section { margin-top: 28px; }
  .sec-head { display: flex; align-items: baseline; gap: 12px; flex-wrap: wrap; margin-bottom: 10px; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 17px; font-weight: 650; display: flex; align-items: center; gap: 8px; }
  h2 :global(svg) { color: var(--accent-2); }
  .hint.note { display: flex; align-items: center; gap: 6px; margin: 10px 2px 0; }

  .table { overflow: hidden; }
  .tr {
    display: grid;
    grid-template-columns: minmax(150px, 2fr) 70px 90px 80px minmax(150px, 1.6fr) 90px 100px;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    font-size: 13.5px;
  }
  .deb .tr { grid-template-columns: minmax(150px, 2fr) 70px 90px 80px minmax(150px, 1.6fr) 100px; }
  .tr:first-child { border-top: none; }
  .tr:not(.th):hover { background: rgba(255, 255, 255, 0.02); }
  .th { padding-top: 10px; padding-bottom: 10px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.07em; color: var(--muted); font-weight: 650; background: rgba(255, 255, 255, 0.015); }
  .r { text-align: right; }
  .num { font-variant-numeric: tabular-nums; }
  .name { display: flex; align-items: center; gap: 8px; min-width: 0; font-weight: 600; }
  .rate { display: flex; align-items: center; gap: 10px; }
  .pct { width: 58px; text-align: right; font-weight: 650; }
  .ok-t { color: #6ee7b7; }
  .err-t { color: #fca5a5; }

  .empty { text-align: center; padding: 34px 20px; color: var(--muted); display: flex; flex-direction: column; align-items: center; gap: 4px; }
  .empty p { margin: 6px 0 0; max-width: 460px; }

  @media (max-width: 980px) {
    .kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }

  @media (max-width: 720px) {
    .view { padding: 16px 14px 28px; }
    h1 { font-size: 23px; }
    .subtitle { font-size: 13px; }
    .toolbar { margin-top: 14px; }
    .seg { max-width: 100%; overflow-x: auto; }
    .seg-btn { padding: 0 10px; }
    .kpis { gap: 8px; margin-top: 14px; }
    .kpi { padding: 12px 14px; }
    .k-value { font-size: 25px; }
    .k-label { font-size: 10px; }
    section { margin-top: 22px; }
    .table { overflow-x: auto; }
    .tr { min-width: 660px; padding: 11px 12px; }
    .tr > :first-child { position: sticky; left: -12px; margin-left: -12px; padding-left: 12px; align-self: stretch; display: flex; align-items: center; background: var(--panel); z-index: 1; box-shadow: 8px 0 12px -8px rgba(0, 0, 0, 0.6); }
    .th > :first-child { background: #12141c; }
  }
</style>
