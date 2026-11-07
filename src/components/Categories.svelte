<script>
  // Ajustes › Categorías: tv, movies… con su carpeta, método, prioridad, post-procesado y las
  // categorías Newznab que les llegan solas. La Default es fija: son los ajustes globales.
  import { t } from '../lib/i18n.svelte.js';
  import { scale } from 'svelte/transition';
  import { getContext } from 'svelte';
  import Icon from './Icon.svelte';
  import Select from './Select.svelte';
  import { ui, saveSettings, ask, d } from '../lib/state.svelte.js';
  import { pickFolder } from '../lib/platform.js';
  import { fold } from '../lib/format.js';
  import { viaOptions } from '../lib/debrid.js';
  import { blankCategory, presetCategories, priorityOptions, PP_LEVELS, ppLabel, categoryDir } from '../lib/categories.js';

  const s = $derived(ui.settings);
  const list = $derived(s?.categories ?? []);
  const vias = $derived([
    { value: '', label: t('cats.viaDefault') },
    ...(s?.featureNntp !== false ? viaOptions(s).map((o) => ({ value: o.value, label: o.child ? `· ${o.label}` : o.label })) : []),
  ]);
  const ppOptions = $derived(PP_LEVELS.map((p) => ({ value: p, label: ppLabel(p) })));
  let newName = $state('');

  const search = getContext('settings-search');
  const miss = $derived(!!search?.q && !fold(`${t('settings.section.categories')} ${t('cats.desc')} ${list.map((c) => c.name).join(' ')}`).includes(search.q));

  function save(next) {
    return saveSettings({ categories: next });
  }

  function update(i, patch) {
    save(list.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  }

  async function add() {
    const name = newName.trim();
    if (!name) return;
    if (await save([...list, blankCategory(name)])) newName = '';
  }

  function addPresets() {
    const have = new Set(list.map((c) => c.name.toLowerCase()));
    save([...list, ...presetCategories().filter((c) => !have.has(c.name))]);
  }

  async function remove(c) {
    if (s.confirmDelete) {
      const r = await ask({ title: t('cats.remove'), message: t('cats.removeConfirm', { name: c.name }), confirmLabel: t('common.remove'), danger: true });
      if (!r?.ok) return;
    }
    save(list.filter((x) => x !== c));
  }

  async function browse(i, c) {
    const dir = await pickFolder({ defaultPath: categoryDir(s, c) || undefined, title: t('cats.dir') });
    if (dir) update(i, { dir });
  }

  function text(i, c, key) {
    return (e) => {
      const v = e.target.value.trim();
      if (v !== (c[key] ?? '')) update(i, { [key]: v });
      // Si el nombre no se acepta (repetido…), el campo vuelve al que había.
      e.target.value = c[key] ?? '';
    };
  }
</script>

<div class="cats srow" class:miss>
  <p class="intro">{t('cats.desc')}</p>

  <div class="card cat def">
    <div class="head">
      <div class="c-icon"><Icon name="inbox" size={16} /></div>
      <div class="name-fixed">
        <b>{t('cats.default')}</b>
        <small>{t('cats.defaultDesc')}</small>
      </div>
    </div>
    <div class="path muted"><Icon name="folder" size={12} /> <span class="truncate">{s.downloadDir}</span></div>
  </div>

  {#each list as c, i (c.name + i)}
    <div class="card cat" in:scale={{ start: 0.97, duration: d(180) }}>
      <div class="head">
        <div class="c-icon"><Icon name="tag" size={16} /></div>
        <input class="input name" value={c.name} onchange={text(i, c, 'name')} aria-label={t('common.name')} spellcheck="false" />
        <button class="btn ghost sm icon" onclick={() => remove(c)} aria-label={t('cats.remove')} title={t('cats.remove')}><Icon name="trash" size={14} /></button>
      </div>
      <div class="fields">
        <label class="f wide">
          <span>{t('cats.dir')}</span>
          <div class="row">
            <input class="input" value={c.dir} placeholder={t('cats.dirPlaceholder')} onchange={text(i, c, 'dir')} spellcheck="false" />
            <button class="btn sm" onclick={() => browse(i, c)}>{t('common.browse')}</button>
          </div>
          <small class="hint truncate">→ {categoryDir(s, c)}</small>
        </label>
        <div class="f">
          <span>{t('cats.via')}</span>
          <Select value={c.via} options={vias} label={t('cats.via')} onchange={(v) => update(i, { via: v })} />
        </div>
        <div class="f">
          <span>{t('cats.priority')}</span>
          <Select value={c.priority} options={priorityOptions()} label={t('cats.priority')} onchange={(v) => update(i, { priority: v })} />
        </div>
        <div class="f">
          <span>{t('cats.ppLabel')}</span>
          <Select value={c.pp} options={ppOptions} label={t('cats.ppLabel')} onchange={(v) => update(i, { pp: v })} />
        </div>
        <label class="f">
          <span>{t('cats.newznab')}</span>
          <input class="input mono" value={c.newznab} placeholder="5000-5999" onchange={text(i, c, 'newznab')} spellcheck="false" />
        </label>
      </div>
    </div>
  {/each}

  <div class="foot">
    <form class="add" onsubmit={(e) => { e.preventDefault(); add(); }}>
      <input class="input" bind:value={newName} placeholder={t('cats.newPlaceholder')} spellcheck="false" />
      <button class="btn sm primary" type="submit" disabled={!newName.trim()}><Icon name="plus" size={14} /> {t('cats.add')}</button>
    </form>
    {#if !list.length}
      <button class="btn sm" onclick={addPresets}><Icon name="sparkles" size={14} /> {t('cats.presets')}</button>
    {/if}
  </div>
  <p class="hint">{t('cats.newznabHelp')}</p>
</div>

<style>
  .cats { display: flex; flex-direction: column; gap: 12px; }
  .cats.miss { display: none; }
  .intro { margin: 0; font-size: 12.5px; color: var(--muted); line-height: 1.5; }
  .cat { padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; }
  .head { display: flex; align-items: center; gap: 8px; }
  .c-icon { width: 32px; height: 32px; border-radius: 9px; display: grid; place-items: center; flex-shrink: 0; background: rgba(var(--accent-rgb), 0.14); color: var(--accent-2); }
  .def .c-icon { background: var(--panel-3); color: var(--text-2); }
  .name-fixed { display: flex; flex-direction: column; min-width: 0; }
  .name-fixed b { font-size: 13.5px; font-weight: 620; }
  .name-fixed small { font-size: 12px; color: var(--muted); }
  .path { display: flex; align-items: center; gap: 6px; font-size: 12px; min-width: 0; }
  .name { flex: 1; min-width: 0; height: 32px; font-weight: 600; }
  .icon { width: 32px; padding: 0; justify-content: center; }
  .fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
  .f { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .f.wide { grid-column: 1 / -1; }
  .f > span { font-size: 12px; color: var(--muted); font-weight: 600; }
  .f .input { height: 32px; font-size: 12.5px; min-width: 0; }
  .row { display: flex; gap: 6px; }
  .row .input { flex: 1; }
  .mono { font-family: var(--mono); }
  .hint { margin: 0; font-size: 12px; color: var(--muted); line-height: 1.5; }
  .foot { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .add { display: flex; gap: 6px; flex: 1 1 260px; }
  .add .input { flex: 1; height: 32px; font-size: 12.5px; }
  @media (max-width: 720px) {
    .fields { grid-template-columns: 1fr; }
  }
</style>
