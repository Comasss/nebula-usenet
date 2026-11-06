<script>
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '../lib/transport.js';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';
  import Switch from './Switch.svelte';
  import { ui, toast } from '../lib/state.svelte.js';
  import * as f from '../lib/format.js';
  import { locale } from '../lib/i18n.svelte.js';

  // `imported`: lo que devuelve import_preview. Todo empieza marcado salvo lo que ya existe.
  let { imported, onclose } = $props();

  const KEYS = ['downloadDir', 'incompleteDir', 'speedLimitKbps', 'precheck', 'duplicateAction', 'par2Enabled', 'extractEnabled', 'directUnpack', 'deleteArchives', 'deletePar2', 'deobfuscate', 'abortHopeless', 'unwantedExtensions', 'unwantedAction'];

  const sameServer = (a, b) => a.host.trim().toLowerCase() === b.host.trim().toLowerCase() && a.port === b.port && a.username.trim() === b.username.trim();
  const serverExists = (s) => (ui.settings.servers ?? []).some((x) => sameServer(x, s));
  const catExists = (c) => (ui.settings.categories ?? []).some((x) => x.name.toLowerCase() === c.name.toLowerCase());
  const settingKeys = Object.keys(imported.settings ?? {}).sort((a, b) => KEYS.indexOf(a) - KEYS.indexOf(b));

  let servers = $state(Object.fromEntries(imported.servers.map((s) => [s.id, !serverExists(s)])));
  let cats = $state(Object.fromEntries(imported.categories.map((c) => [c.name, !catExists(c)])));
  let settings = $state(Object.fromEntries(settingKeys.map((k) => [k, true])));
  let schedule = $state(imported.schedule.length > 0);
  let busy = $state(false);

  const count = $derived(Object.values(servers).filter(Boolean).length + Object.values(cats).filter(Boolean).length + Object.values(settings).filter(Boolean).length + (schedule ? imported.schedule.length : 0));

  function label(k) {
    return KEYS.includes(k) ? t(`import.key.${k}`) : k;
  }

  function value(k, v) {
    if (typeof v === 'boolean') return v ? t('import.yes') : t('import.no');
    if (k === 'speedLimitKbps') return v ? f.speed(v * 1024) : t('import.noLimit');
    if (k === 'unwantedAction') return t(`settings.unwantedAction.${v}`);
    if (k === 'duplicateAction') return t(`settings.duplicateAction.${v}`);
    if (Array.isArray(v)) return v.join(' ');
    return String(v);
  }

  function rule(r) {
    const what = r.action === 'speed' ? (r.value ? f.speed(r.value * 1024) : t('import.noLimit')) : t(`sched.${r.action}`);
    const days = r.days.length ? r.days.map((n) => new Intl.DateTimeFormat(locale(), { weekday: 'short' }).format(new Date(2024, 0, n))).join(' ') : t('import.everyDay');
    return `${r.time} · ${what} · ${days}`;
  }

  async function apply() {
    busy = true;
    try {
      const choice = {
        servers: Object.keys(servers).filter((k) => servers[k]),
        categories: Object.keys(cats).filter((k) => cats[k]),
        settings: Object.keys(settings).filter((k) => settings[k]),
        schedule,
      };
      const done = await invoke('import_apply', { imported, choice });
      ui.settings = await invoke('get_settings');
      toast('success', t('import.done', { servers: done.servers, categories: done.categories, settings: done.settings, schedule: done.schedule }));
      onclose();
    } catch (e) {
      toast('error', String(e));
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={t('import.title', { source: imported.source === 'nzbget' ? 'NZBGet' : 'SABnzbd' })} subtitle={t('import.subtitle')} icon="download" width={560} {onclose} dismissable={!busy}>
  <div class="imp">
    {#if imported.warnings.length}
      <div class="alert warn">
        <Icon name="alert-triangle" size={15} />
        <ul>{#each imported.warnings as w}<li>{w}</li>{/each}</ul>
      </div>
    {/if}

    {#if imported.servers.length}
      <h3>{t('import.servers')}</h3>
      {#each imported.servers as s (s.id)}
        <div class="row">
          <div class="main">
            <div class="name">{s.name || s.host}{#if !s.enabled}<span class="chip muted">{t('import.disabled')}</span>{/if}{#if serverExists(s)}<span class="chip warn">{t('import.exists')}</span>{/if}</div>
            <div class="sub">{s.host}:{s.port}{s.ssl ? ' · SSL' : ''} · {t('import.conns', { n: s.connections })}{s.quotaGb ? ` · ${s.quotaGb} GB` : ''}</div>
          </div>
          <Switch checked={servers[s.id]} disabled={serverExists(s)} onchange={(v) => (servers[s.id] = v)} label={s.host} />
        </div>
      {/each}
    {/if}

    {#if imported.categories.length}
      <h3>{t('import.categories')}</h3>
      {#each imported.categories as c (c.name)}
        <div class="row">
          <div class="main">
            <div class="name">{c.name}{#if catExists(c)}<span class="chip warn">{t('import.exists')}</span>{/if}</div>
            {#if c.dir || c.newznab}<div class="sub">{[c.dir, c.newznab].filter(Boolean).join(' · ')}</div>{/if}
          </div>
          <Switch checked={cats[c.name]} disabled={catExists(c)} onchange={(v) => (cats[c.name] = v)} label={c.name} />
        </div>
      {/each}
    {/if}

    {#if settingKeys.length}
      <h3>{t('import.settings')}</h3>
      {#each settingKeys as k (k)}
        <div class="row">
          <div class="main">
            <div class="name">{label(k)}</div>
            <div class="sub mono">{value(k, imported.settings[k])}</div>
          </div>
          <Switch checked={settings[k]} onchange={(v) => (settings[k] = v)} label={label(k)} />
        </div>
      {/each}
    {/if}

    {#if imported.schedule.length}
      <h3>{t('import.schedule')}</h3>
      <div class="row">
        <div class="main">
          {#each imported.schedule as r}<div class="sub">{rule(r)}{#if !r.enabled} <span class="chip muted">{t('import.disabled')}</span>{/if}</div>{/each}
        </div>
        <Switch checked={schedule} onchange={(v) => (schedule = v)} label={t('import.schedule')} />
      </div>
    {/if}
  </div>

  {#snippet footer()}
    <button class="btn ghost" onclick={onclose} disabled={busy}>{t('common.cancel')}</button>
    <button class="btn primary" onclick={apply} disabled={busy || !count}><Icon name="download" size={15} /> {t('import.apply', { n: count })}</button>
  {/snippet}
</Modal>

<style>
  .imp { display: flex; flex-direction: column; gap: 6px; }
  h3 { margin: 10px 0 2px; font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); }
  .row { display: flex; align-items: center; gap: 12px; padding: 8px 10px; border-radius: 10px; background: var(--panel-2); }
  .main { flex: 1; min-width: 0; }
  .name { font-size: 13.5px; font-weight: 500; display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .sub { font-size: 12px; color: var(--muted); overflow-wrap: anywhere; }
  .mono { font-family: var(--mono, ui-monospace, monospace); }
  .alert ul { margin: 0; padding-left: 16px; display: flex; flex-direction: column; gap: 3px; }
  .alert { align-items: flex-start; }
</style>
