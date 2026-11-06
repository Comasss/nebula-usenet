<script>
  import { t, tn } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import { remote as rc, call } from '../lib/remote.svelte.js';
  import { slide } from 'svelte/transition';
  import Icon from './Icon.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import { ui, ask, toast, d } from '../lib/state.svelte.js';
  import * as f from '../lib/format.js';
  import { isMobile, isServer } from '../lib/platform.js';
  import { haptic, longpress, actionSheet } from '../lib/feedback.js';
  import Select from './Select.svelte';
  import { catLabel, isDefault, categoryOptions, priorityOptions, priorityLabel, PRIORITIES } from '../lib/categories.js';

  let { job, index, count } = $props();
  let expanded = $state(false);
  let files = $state([]);

  const st = $derived(f.STATUS[job.status] ?? f.STATUS.queued);
  const pp = $derived(['verifying', 'repairing', 'extracting', 'moving'].includes(job.status));
  const percent = $derived(pp && job.post ? job.post.percent : job.percent);
  const rate = $derived(pp ? job.post?.speed ?? 0 : job.speed);
  // Descarga debrid mientras el servicio aún la tiene (antes de bajar los archivos al PC).
  const remote = $derived(!!job.debrid && job.debrid.phase !== 'fetch' && !pp);
  const barKind = $derived(
    remote && job.status === 'downloading'
      ? 'info'
      : ({ downloading: 'accent', queued: 'muted', paused: 'paused', verifying: 'info', repairing: 'warn', extracting: 'teal', moving: 'teal' }[job.status] ?? 'accent')
  );
  const waiting = $derived(pp && job.post?.stage === 'waiting');
  // Debrid: una barra por fase (subida del NZB, descarga en el servicio y bajada al dispositivo).
  // NZB ya subido: el servicio lo está aceptando y no hay progreso que mostrar.
  const accepting = $derived(!!job.debrid?.waiting && !pp);
  const steps = $derived.by(() => {
    if (!job.debrid || pp) return [];
    const ph = job.debrid.phase;
    return [
      { label: t('job.stepUpload'), value: job.debrid.upload ?? (ph === 'submit' ? 0 : 100), current: ph === 'submit' && !accepting },
      {
        label: t('job.stepRemote', { provider: job.debrid.provider }),
        value: job.debrid.remote ?? (ph === 'fetch' ? 100 : 0),
        current: ph === 'remote' || accepting,
        indeterminate: accepting,
      },
      { label: t('job.stepFetch'), value: ph === 'fetch' ? job.percent : 0, current: ph === 'fetch' },
    ];
  });

  $effect(() => {
    if (!expanded) return;
    let alive = true;
    const load = async () => {
      if (!alive) return;
      try {
        files = await call('get_job_files', { id: job.id });
      } catch {}
    };
    load();
    const timer = setInterval(load, 1500);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  function togglePause() {
    haptic('tap');
    call(job.status === 'paused' ? 'resume_job' : 'pause_job', { id: job.id });
  }

  // Móvil: todas las acciones en una hoja (botón ⋯ o mantener pulsada la tarjeta).
  async function openMenu() {
    const paused = job.status === 'paused';
    const actions = [
      ...(!pp
        ? [
            { id: 'pause', label: paused ? t('common.resume') : t('common.pause'), icon: paused ? 'play' : 'pause' },
            { id: 'top', label: t('job.moveTop'), icon: 'arrow-up-to-line', disabled: index === 0 },
            { id: 'up', label: t('job.moveUp'), icon: 'chevron-up', disabled: index === 0 },
            { id: 'down', label: t('job.moveDown'), icon: 'chevron-down', disabled: index >= count - 1 },
            { id: 'priority', label: `${t('cats.priority')}: ${priorityLabel(job.priority ?? 0)}`, icon: 'zap' },
          ]
        : []),
      { id: 'details', label: expanded ? t('job.hideDetails') : t('common.details'), icon: 'info' },
      ...(!rc.active && !isServer ? [{ id: 'open', label: t('job.openDest'), icon: 'folder' }] : []),
      { id: 'remove', label: t('common.remove'), icon: 'trash', danger: true },
    ];
    const a = await actionSheet({ title: job.name, subtitle: `${f.bytes(job.total)} · ${st.label}`, actions });
    if (a === 'pause') togglePause();
    else if (a === 'top') move(0);
    else if (a === 'up') move(index - 1);
    else if (a === 'down') move(index + 1);
    else if (a === 'priority') {
      const p = await actionSheet({
        title: t('cats.priority'),
        subtitle: job.name,
        actions: PRIORITIES.map((v) => ({ id: String(v), label: priorityLabel(v), icon: v === (job.priority ?? 0) ? 'check' : 'zap' })),
      });
      if (p !== null && p !== undefined) setPriority(Number(p));
    }
    else if (a === 'details') expanded = !expanded;
    else if (a === 'open') invoke('open_path', { path: job.dest });
    else if (a === 'remove') remove();
  }

  async function remove() {
    let deleteFiles = true;
    if (ui.settings?.confirmDelete) {
      const r = await ask({
        title: t('job.removeTitle'),
        message: job.name,
        confirmLabel: t('common.remove'),
        danger: true,
        icon: 'trash',
        checkbox: t('job.removeFiles'),
        checkedDefault: true,
      });
      if (!r.ok) return;
      deleteFiles = r.checked;
    }
    await call('remove_job', { id: job.id, deleteFiles });
    toast('info', t('job.removed', { name: job.name }));
  }

  function move(to) {
    call('move_job', { id: job.id, to });
  }

  function setPriority(priority) {
    if (priority !== (job.priority ?? 0)) call('set_job_priority', { id: job.id, priority });
  }

  // Las categorías de un PC controlado no se conocen aquí: allí solo se cambia la prioridad.
  const catOptions = $derived(rc.active ? [] : categoryOptions(ui.settings));

  // Fase del post-procesado (clave de traducción; las colas antiguas pueden traer el texto ya escrito).
  function stageText(stage) {
    const key = `stage.${stage}`;
    const text = t(key);
    return text === key ? stage : text;
  }

  function subline() {
    if (pp) {
      const stage = job.post?.stage ? stageText(job.post.stage) : st.label;
      return job.post?.detail ? `${stage} · ${job.post.detail}` : stage;
    }
    if (remote) return job.status === 'paused' ? `${t('status.paused')} · ${job.debrid.state}` : job.debrid.state;
    const parts = [t('job.progress', { done: f.bytes(job.done + job.failed), total: f.bytes(job.total) })];
    parts.push(t('job.filesProgress', { done: job.filesDone, total: job.files }));
    return parts.join(' · ');
  }
</script>

<article class="job card {job.status}" class:expanded use:longpress={{ enabled: isMobile, onlong: openMenu, ms: 520 }}>
  <div class="top">
    <div class="s-icon {st.kind}" class:spinning={pp && !waiting}>
      {#if pp && !waiting}
        <svg class="ring" viewBox="0 0 44 44"><circle cx="22" cy="22" r="20" /></svg>
      {/if}
      <Icon name={st.icon} size={18} />
    </div>

    <div class="main">
      <div class="name truncate" title={job.name}>{job.name}</div>
      <div class="meta">
        <span class="chip {st.kind}">{waiting ? t('stage.waiting') : st.label}</span>
        {#if job.debrid}
          <span class="chip info" title={t('job.viaTitle', { provider: job.debrid.provider })}><Icon name="cloud" size={11} stroke={2.4} /> {job.debrid.provider}</span>
        {/if}
        {#if job.server}
          <span class="chip accent" title={t('job.serverTitle')}><Icon name="server" size={11} stroke={2.4} /> {job.server}</span>
        {/if}
        {#if !isDefault(job.category)}
          <span class="chip cat" title={t('cats.category')}><Icon name="tag" size={11} stroke={2.4} /> {job.category}</span>
        {/if}
        {#if job.priority && job.priority !== 0}
          <span class="chip {job.priority > 0 ? 'warn' : 'muted'}" title={t('cats.priority')}><Icon name="zap" size={11} stroke={2.4} /> {priorityLabel(job.priority)}</span>
        {/if}
        <span class="sub truncate">{subline()}</span>
        {#if job.heldPar2 > 0 && ['queued', 'downloading', 'paused'].includes(job.status)}
          <span class="chip muted" title={t('job.heldPar2Title', { n: job.heldPar2 })}>
            <Icon name="shield" size={11} stroke={2.4} /> {t('job.heldPar2')}
          </span>
        {/if}
        {#if job.failedArticles > 0}
          <span class="chip warn" title={t('job.missingTitle')}>
            <Icon name="alert-triangle" size={11} stroke={2.5} />
            {t('job.missing', { n: job.failedArticles })}
          </span>
        {/if}
      </div>
    </div>

    <div class="stats">
      {#if rate > 0}
        <div class="rate" title={remote ? t('job.remoteSpeed', { provider: job.debrid.provider }) : ''}>{f.speed(rate)}</div>
      {/if}
      {#if job.status === 'downloading' && job.eta > 0}
        <div class="eta"><Icon name="clock" size={12} /> {f.duration(job.eta)}</div>
      {:else if remote && job.status === 'downloading'}
        <div class="eta"><Icon name="cloud" size={12} /> {t('job.atProvider', { provider: job.debrid.provider })}</div>
      {:else if job.status === 'queued'}
        <div class="eta">{t('job.queuePos', { n: index + 1 })}</div>
      {/if}
    </div>

    {#if isMobile}
      <div class="m-actions">
        {#if !pp}
          <button class="m-btn" class:resume={job.status === 'paused'} onclick={togglePause}>
            <Icon name={job.status === 'paused' ? 'play' : 'pause'} size={15} stroke={2.4} />
            {job.status === 'paused' ? t('common.resume') : t('common.pause')}
          </button>
        {/if}
        <button class="icon-btn chevron" class:open={expanded} onclick={() => { haptic('tap'); expanded = !expanded; }} aria-label={t('common.details')}><Icon name="chevron-down" size={18} /></button>
        <button class="icon-btn" onclick={openMenu} aria-label={t('job.more')}><Icon name="more" size={18} stroke={2.6} /></button>
      </div>
    {:else}
    <div class="actions">
      {#if !pp}
        <button class="icon-btn" onclick={() => move(0)} disabled={index === 0} title={t('job.moveTop')}><Icon name="arrow-up-to-line" size={16} /></button>
        <button class="icon-btn" onclick={() => move(index - 1)} disabled={index === 0} title={t('job.moveUp')}><Icon name="chevron-up" size={17} /></button>
        <button class="icon-btn" onclick={() => move(index + 1)} disabled={index >= count - 1} title={t('job.moveDown')}><Icon name="chevron-down" size={17} /></button>
        <button class="icon-btn" onclick={togglePause} title={job.status === 'paused' ? t('common.resume') : t('common.pause')}>
          <Icon name={job.status === 'paused' ? 'play' : 'pause'} size={16} />
        </button>
      {/if}
      {#if !rc.active && !isServer}<button class="icon-btn" onclick={() => invoke('open_path', { path: job.dest })} title={t('job.openDest')}><Icon name="folder" size={16} /></button>{/if}
      <button class="icon-btn danger" onclick={remove} title={t('common.remove')}><Icon name="trash" size={16} /></button>
      <button class="icon-btn chevron" class:open={expanded} onclick={() => (expanded = !expanded)} title={t('common.details')}><Icon name="chevron-down" size={17} /></button>
    </div>
    {/if}
  </div>

  <div class="bar-row">
    <ProgressBar value={percent} kind={barKind} indeterminate={accepting && job.status === 'downloading'} active={(job.status === 'downloading' && !ui.queue.paused) || (pp && !waiting)} />
    <span class="pct">{accepting ? '…' : `${percent.toFixed(1)}%`}</span>
  </div>

  {#if steps.length}
    <div class="steps">
      {#each steps as s}
        <div class="step" class:current={s.current}>
          <div class="step-head"><span class="truncate">{s.label}</span><span class="step-pct">{s.indeterminate ? '…' : `${s.value.toFixed(0)}%`}</span></div>
          <ProgressBar
            value={s.value}
            height={4}
            kind={s.current ? barKind : s.value >= 100 ? 'ok' : 'muted'}
            indeterminate={s.indeterminate && job.status === 'downloading'}
            active={s.current && job.status === 'downloading' && !ui.queue.paused}
          />
        </div>
      {/each}
    </div>
  {/if}

  {#if job.error}
    <div class="alert err" transition:slide={{ duration: d(200) }}><Icon name="x-circle" size={16} /><span class="selectable">{job.error}</span></div>
  {/if}
  {#if job.warning}
    <div class="alert warn" transition:slide={{ duration: d(200) }}><Icon name="alert-triangle" size={16} /><span class="selectable">{job.warning}</span></div>
  {/if}
  {#if job.status === 'downloading' && ui.queue.noServers && !job.debrid}
    <div class="alert warn" transition:slide={{ duration: d(200) }}>
      <Icon name="wifi" size={16} /><span>{t('job.waitingServer')}</span>
    </div>
  {/if}

  {#if expanded}
    <div class="details" transition:slide={{ duration: d(260) }}>
      <div class="d-grid">
        <div><span class="k">{t('job.destination')}</span><span class="v truncate selectable" title={job.dest}>{job.dest}</span></div>
        <div><span class="k">{t('common.size')}</span><span class="v">{f.bytes(job.total)}</span></div>
        <div><span class="k">{t('common.added')}</span><span class="v">{f.ago(job.addedAt)}</span></div>
        {#if job.debrid}
          <div><span class="k">{t('job.method')}</span><span class="v">{job.debrid.provider} (debrid)</span></div>
        {:else if job.server}
          <div><span class="k">{t('job.method')}</span><span class="v">{job.server} (NNTP)</span></div>
        {/if}
        {#if job.heldPar2 > 0}
          <div><span class="k">{t('job.heldPar2')}</span><span class="v">{tn('job.heldVolumes', job.heldPar2)}</span></div>
        {/if}
        {#if !pp}
          {#if catOptions.length > 1}
            <div><span class="k">{t('cats.category')}</span><Select value={catOptions.some((o) => o.value === job.category) ? job.category : '*'} options={catOptions} label={t('cats.category')} onchange={(v) => call('set_job_category', { id: job.id, category: v })} /></div>
          {:else if !isDefault(job.category)}
            <div><span class="k">{t('cats.category')}</span><span class="v">{catLabel(job.category)}</span></div>
          {/if}
          <div><span class="k">{t('cats.priority')}</span><Select value={job.priority ?? 0} options={priorityOptions()} label={t('cats.priority')} onchange={setPriority} /></div>
        {/if}
      </div>
      {#if files.length}
        <div class="files">
          {#each files as file (file.name)}
            {@const fp = file.bytes ? ((file.done + file.failed) / file.bytes) * 100 : 0}
            <div class="file" class:held={file.held}>
              <Icon name={file.par2 ? 'shield' : 'file'} size={14} />
              <span class="fname truncate" title={file.name}>{file.name}</span>
              <span>
                {#if file.held}
                  <span class="chip muted">{t('job.held')}</span>
                {:else if file.failedSegs > 0}
                  <span class="chip warn">{t('job.missing', { n: file.failedSegs })}</span>
                {/if}
              </span>
              <span class="fsize">{f.bytes(file.bytes)}</span>
              <div class="fbar"><ProgressBar value={file.held ? 0 : fp} kind={file.complete ? (file.failedSegs ? 'warn' : 'ok') : 'accent'} height={4} /></div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</article>

<style>
  .job {
    padding: 16px 16px 14px 16px;
    transition: border-color 0.25s, box-shadow 0.3s, transform 0.2s var(--ease);
  }
  .job:hover { border-color: var(--border-2); }
  .job.downloading { border-color: rgba(var(--accent-rgb), 0.28); box-shadow: 0 10px 30px -18px rgba(var(--accent-rgb), 0.8); }
  .top { display: flex; align-items: center; gap: 14px; }
  .s-icon {
    position: relative;
    width: 42px; height: 42px;
    border-radius: 12px;
    display: grid; place-items: center;
    flex-shrink: 0;
    background: var(--panel-3);
    color: var(--text-2);
  }
  .s-icon.accent { background: rgba(var(--accent-rgb), 0.16); color: var(--accent-2); }
  .s-icon.info { background: rgba(96, 165, 250, 0.14); color: var(--info); }
  .s-icon.warn { background: rgba(251, 191, 36, 0.14); color: var(--warn); }
  .s-icon.teal { background: rgba(45, 212, 191, 0.14); color: #2dd4bf; }
  .s-icon.err { background: rgba(248, 113, 113, 0.14); color: var(--err); }
  .downloading .s-icon :global(svg) { animation: drip 1.6s ease-in-out infinite; }
  @keyframes drip { 50% { transform: translateY(2px); } }
  .ring { position: absolute; inset: -1px; width: 44px; height: 44px; animation: spin 1.4s linear infinite; }
  .ring circle { fill: none; stroke: currentColor; stroke-width: 2; stroke-dasharray: 30 100; stroke-linecap: round; opacity: 0.8; }
  .main { flex: 1; min-width: 0; }
  .name { font-weight: 620; font-size: 14.5px; }
  .meta { display: flex; align-items: center; gap: 8px; margin-top: 5px; min-width: 0; }
  .sub { font-size: 12.5px; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .stats { text-align: right; flex-shrink: 0; min-width: 90px; }
  .rate { font-family: var(--font-display); font-weight: 650; font-size: 15px; font-variant-numeric: tabular-nums; }
  .eta { display: inline-flex; align-items: center; gap: 4px; font-size: 12px; color: var(--muted); margin-top: 2px; font-variant-numeric: tabular-nums; }
  .actions { display: flex; gap: 2px; flex-shrink: 0; }
  .actions .icon-btn { opacity: 0.75; }
  .job:hover .actions .icon-btn { opacity: 1; }
  .chevron :global(svg) { transition: transform 0.3s var(--ease); }
  .chevron.open :global(svg) { transform: rotate(180deg); }
  .bar-row { display: flex; align-items: center; gap: 12px; margin-top: 13px; }
  .pct { width: 52px; text-align: right; font-size: 12.5px; font-weight: 650; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .steps { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin-top: 10px; padding-right: 64px; }
  .step-head { display: flex; justify-content: space-between; gap: 6px; margin-bottom: 4px; font-size: 11px; font-weight: 600; color: var(--muted); }
  .step.current .step-head { color: var(--text-2); }
  .step-pct { font-variant-numeric: tabular-nums; }
  .alert { margin-top: 12px; }
  .details { margin-top: 14px; padding-top: 14px; border-top: 1px solid var(--border); }
  .d-grid { display: grid; grid-template-columns: 2fr 1fr 1fr; gap: 10px 18px; margin-bottom: 12px; }
  .d-grid > div { display: flex; flex-direction: column; min-width: 0; }
  .k { font-size: 11px; text-transform: uppercase; letter-spacing: 0.07em; color: var(--muted); font-weight: 650; }
  .v { font-size: 13px; margin-top: 2px; }
  .files { max-height: 260px; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; margin: 0 -6px; padding: 0 6px; }
  .file { display: grid; grid-template-columns: 16px 1fr auto 76px 120px; align-items: center; gap: 10px; padding: 6px 8px; border-radius: 8px; font-size: 12.5px; color: var(--text-2); }
  .file:hover { background: var(--panel-2); }
  .file.held { opacity: 0.55; }
  .fname { color: var(--text); }
  .fsize { text-align: right; font-variant-numeric: tabular-nums; }

  .m-actions { order: 5; width: 100%; display: flex; align-items: center; gap: 6px; padding-top: 10px; border-top: 1px solid rgba(255, 255, 255, 0.05); }
  .m-btn {
    display: inline-flex; align-items: center; gap: 7px;
    height: 36px; padding: 0 14px; margin-right: auto;
    border-radius: 999px; border: 1px solid var(--border-2);
    background: var(--panel-2); color: var(--text);
    font-size: 13px; font-weight: 620; cursor: pointer;
    transition: transform 0.15s var(--ease), background 0.2s;
  }
  .m-btn:active { transform: scale(0.94); }
  .m-btn.resume { border-color: transparent; background: var(--grad-diag); color: #fff; box-shadow: 0 6px 18px -8px rgba(var(--accent-rgb), 0.9); }
  :global(.job.holding) { transform: scale(0.985); }

  /* Móvil: las acciones bajan a su propia fila y la información se reparte en varias líneas. */
  @media (max-width: 720px) {
    .job { padding: 14px 14px 12px; }
    .top { flex-wrap: wrap; gap: 10px 12px; }
    .s-icon { width: 38px; height: 38px; }
    .ring { width: 40px; height: 40px; }
    .name { white-space: normal; overflow: visible; word-break: break-all; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; font-size: 14px; }
    .meta { flex-wrap: wrap; row-gap: 6px; }
    .stats { min-width: 0; }
    .actions { order: 5; width: 100%; justify-content: space-between; padding-top: 2px; border-top: 1px solid var(--border); margin-top: 2px; }
    .actions .icon-btn { opacity: 1; }
    .bar-row { margin-top: 10px; }
    .steps { padding-right: 0; gap: 10px; }
    .d-grid { grid-template-columns: 1fr 1fr; }
    .d-grid > div:first-child { grid-column: span 2; }
    .file { grid-template-columns: 16px 1fr auto 64px; row-gap: 4px; }
    .fbar { grid-column: 1 / -1; }
  }
</style>
