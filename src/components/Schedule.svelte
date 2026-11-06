<script>
  // Programador: tareas que pausan, reanudan o limitan la velocidad a una hora (hora local del
  // equipo donde corre el motor). Días vacíos = todos.
  import { slide } from 'svelte/transition';
  import { t, locale } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import Select from './Select.svelte';
  import { ui, saveSettings, d } from '../lib/state.svelte.js';

  const rules = $derived(ui.settings?.schedule ?? []);
  // 2024-01-01 fue lunes: el motor numera los días de 1 (lunes) a 7 (domingo).
  const DAYS = $derived(
    [1, 2, 3, 4, 5, 6, 7].map((n) => ({
      n,
      short: new Intl.DateTimeFormat(locale(), { weekday: 'narrow' }).format(new Date(2024, 0, n)),
      long: new Intl.DateTimeFormat(locale(), { weekday: 'long' }).format(new Date(2024, 0, n)),
    }))
  );
  const ACTIONS = $derived([
    { value: 'pause', label: t('sched.pause') },
    { value: 'resume', label: t('sched.resume') },
    { value: 'speed', label: t('sched.speed') },
  ]);

  function save(list) {
    saveSettings({ schedule: list });
  }
  function update(i, patch) {
    save(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  }
  function toggleDay(i, n) {
    const days = rules[i].days ?? [];
    update(i, { days: days.includes(n) ? days.filter((x) => x !== n) : [...days, n].sort() });
  }
  function add() {
    const id = Math.random().toString(16).slice(2, 10);
    save([...rules, { id, enabled: true, days: [], time: '01:00', action: 'pause', value: 0 }]);
  }
  const mbs = (kbps) => (kbps ? +(kbps / 1024).toFixed(1) : 0);
</script>

<div class="sched">
  {#each rules as r, i (r.id)}
    <div class="rule" class:off={!r.enabled} transition:slide={{ duration: d(180) }}>
      <Switch checked={r.enabled} onchange={(v) => update(i, { enabled: v })} label={t('sched.enabled')} />
      <input class="input time" type="time" value={r.time} onchange={(e) => e.target.value && update(i, { time: e.target.value })} aria-label={t('sched.time')} />
      <div class="days" role="group" aria-label={t('sched.days')}>
        {#each DAYS as day}
          <button class="day" class:on={!r.days?.length || r.days.includes(day.n)} class:all={!r.days?.length} title={day.long} onclick={() => toggleDay(i, day.n)}>{day.short}</button>
        {/each}
      </div>
      <div class="act"><Select value={r.action} options={ACTIONS} onchange={(v) => update(i, { action: v })} label={t('sched.action')} /></div>
      {#if r.action === 'speed'}
        <span class="speed">
          <input class="input small" type="number" min="0" step="0.5" value={mbs(r.value)} aria-label={t('downloads.speedLimit')}
            onchange={(e) => update(i, { value: Math.round(Math.max(0, parseFloat(String(e.target.value).replace(',', '.')) || 0) * 1024) })} />
          <span class="unit">MB/s</span>
        </span>
      {/if}
      <button class="btn ghost sm icon del" onclick={() => save(rules.filter((_, j) => j !== i))} aria-label={t('common.remove')} title={t('common.remove')}><Icon name="trash" size={14} /></button>
    </div>
  {:else}
    <div class="empty">{t('sched.empty')}</div>
  {/each}
  <div class="foot">
    <button class="btn sm" onclick={add}><Icon name="plus" size={14} /> {t('sched.add')}</button>
    <span class="hint">{t('sched.hint')}</span>
  </div>
</div>

<style>
  .sched { padding: 6px 0; }
  .rule { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; padding: 10px 18px; border-bottom: 1px solid var(--border); transition: opacity 0.2s; }
  .rule.off { opacity: 0.55; }
  .time { width: 128px; height: 32px; }
  .act { width: 172px; }
  .days { display: flex; gap: 3px; }
  .day {
    width: 26px; height: 28px; border-radius: 8px; border: 1px solid var(--border-2);
    background: var(--panel-2); color: var(--muted); font-size: 12px; font-weight: 650; cursor: pointer; text-transform: uppercase;
  }
  .day.on { background: color-mix(in srgb, var(--accent) 22%, transparent); border-color: color-mix(in srgb, var(--accent) 55%, transparent); color: var(--text); }
  .day.on.all { background: var(--panel-3); border-color: var(--border-2); }
  .speed { display: inline-flex; align-items: center; gap: 6px; }
  .speed .input { width: 74px; }
  .unit { font-size: 12px; color: var(--muted); }
  .del { margin-left: auto; }
  .empty { padding: 14px 18px; color: var(--muted); font-size: 13px; border-bottom: 1px solid var(--border); }
  .foot { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; padding: 12px 18px 8px; }
  .hint { font-size: 12px; color: var(--muted); }
  @media (max-width: 700px) {
    .rule { padding: 12px 14px; }
    .foot { padding: 12px 14px 6px; }
  }
</style>
