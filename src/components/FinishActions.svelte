<script>
  // Ajustes › Al terminar: acciones que se ejecutan cuando una descarga termina, falla o la cola
  // se queda vacía. En Android solo hay notificaciones; la energía solo existe en Windows.
  import { t } from '../lib/i18n.svelte.js';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { scale } from 'svelte/transition';
  import { getContext } from 'svelte';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';
  import { ui, saveSettings, toast, ask, d } from '../lib/state.svelte.js';
  import { isAndroid, isWindows } from '../lib/platform.js';
  import { fold } from '../lib/format.js';

  const NOTIFY = ['discord', 'telegram', 'ntfy', 'webhook'];
  const KINDS = [...NOTIFY, ...(isAndroid ? [] : ['script']), ...(isWindows ? ['power'] : [])];
  const ICONS = { discord: 'message', telegram: 'send', ntfy: 'bell', webhook: 'plug', script: 'terminal', power: 'power' };

  const s = $derived(ui.settings);
  // En Android no se muestran los scripts ni la energía que lleguen de otro equipo (ni se borran).
  const list = $derived((s?.finishActions ?? []).filter((a) => KINDS.includes(a.kind)));
  let adding = $state(false);
  let testing = $state(null);
  let shown = $state({});

  // Buscador de Ajustes: la sección se ve si coincide con el título, o con alguna acción.
  const search = getContext('settings-search');
  const miss = $derived(!!search?.q && !fold(`${t('finish.title')} ${t('finish.desc')} ${KINDS.map((k) => t(`finish.kind.${k}`)).join(' ')}`).includes(search.q));

  function newId() {
    return crypto.randomUUID?.() ?? String(Date.now()) + Math.random().toString(16).slice(2);
  }

  function blank(kind) {
    const power = kind === 'power';
    return {
      id: newId(), kind, name: '', enabled: true,
      onComplete: !power, onFailed: !power, onQueueEmpty: power,
      url: '', token: '', chatId: '', topic: '', path: '', timeoutSecs: 120, power: 'shutdown', once: false,
    };
  }

  function save(next) {
    return saveSettings({ finishActions: next });
  }

  function update(id, patch) {
    save((s.finishActions ?? []).map((a) => (a.id === id ? { ...a, ...patch } : a)));
  }

  function add(kind) {
    adding = false;
    save([...(s.finishActions ?? []), blank(kind)]);
  }

  function label(a) {
    if (a.name?.trim()) return a.name.trim();
    if (a.kind === 'power') return t(`finish.power.${a.power || 'shutdown'}`);
    if (a.kind === 'script' && a.path) return a.path.split(/[\\/]/).pop();
    return t(`finish.kind.${a.kind}`);
  }

  async function remove(a) {
    if (s.confirmDelete) {
      const r = await ask({ title: t('finish.remove'), message: t('finish.removeConfirm', { name: label(a) }), confirmLabel: t('common.remove'), danger: true });
      if (!r?.ok) return;
    }
    save((s.finishActions ?? []).filter((x) => x.id !== a.id));
  }

  async function test(a) {
    testing = a.id;
    try {
      await invoke('finish_test', { action: $state.snapshot(a) });
      toast('success', a.kind === 'power' ? t('finish.testPower') : t('finish.testOk', { name: label(a) }), 3500);
    } catch (e) {
      toast('error', `${label(a)}: ${e}`);
    } finally {
      testing = null;
    }
  }

  async function pickScript(a) {
    const path = await open({
      title: t('finish.pickScript'),
      defaultPath: a.path || undefined,
      filters: isWindows
        ? [{ name: t('finish.programs'), extensions: ['exe', 'bat', 'cmd', 'ps1', 'com'] }, { name: '*', extensions: ['*'] }]
        : [],
    });
    if (typeof path === 'string') update(a.id, { path });
  }

  function text(a, key) {
    return (e) => {
      const v = e.target.value.trim();
      if (v !== (a[key] ?? '')) update(a.id, { [key]: v });
    };
  }

  function placeholder(kind) {
    if (kind === 'discord') return 'https://discord.com/api/webhooks/…';
    if (kind === 'ntfy') return 'https://ntfy.sh';
    return 'https://…';
  }
</script>

<svelte:window onclick={() => (adding = false)} />

<div class="fa srow" class:miss>
  <p class="intro">{t('finish.desc')}</p>

  {#each list as a (a.id)}
    {@const power = a.kind === 'power'}
    <div class="card act" class:off={!a.enabled} in:scale={{ start: 0.97, duration: d(180) }}>
      <div class="head">
        <div class="k-icon k-{a.kind}"><Icon name={ICONS[a.kind]} size={17} /></div>
        <input class="input name" value={a.name} placeholder={label({ ...a, name: '' })} onchange={text(a, 'name')} aria-label={t('common.name')} />
        <button class="btn sm" onclick={() => test(a)} disabled={testing === a.id}>
          <Icon name={testing === a.id ? 'loader' : 'zap'} size={13} /> {testing === a.id ? t('common.testing') : t('common.test')}
        </button>
        <button class="btn ghost sm icon" onclick={() => remove(a)} aria-label={t('finish.remove')} title={t('finish.remove')}><Icon name="trash" size={14} /></button>
        <Switch checked={a.enabled} label={t('common.enabled')} onchange={(v) => update(a.id, { enabled: v })} />
      </div>

      <div class="when">
        <span class="w-label">{t('finish.when')}</span>
        {#if power}
          <span class="chip on fixed"><Icon name="inbox" size={12} /> {t('finish.on.queueEmpty')}</span>
        {:else}
          <button class="chip" class:on={a.onComplete} onclick={() => update(a.id, { onComplete: !a.onComplete })}><Icon name="check" size={12} /> {t('finish.on.complete')}</button>
          <button class="chip" class:on={a.onFailed} onclick={() => update(a.id, { onFailed: !a.onFailed })}><Icon name="x" size={12} /> {t('finish.on.failed')}</button>
          <button class="chip" class:on={a.onQueueEmpty} onclick={() => update(a.id, { onQueueEmpty: !a.onQueueEmpty })}><Icon name="inbox" size={12} /> {t('finish.on.queueEmpty')}</button>
        {/if}
      </div>

      <div class="fields">
        {#if a.kind === 'discord' || a.kind === 'webhook'}
          <label class="f wide">
            <span>{a.kind === 'discord' ? t('finish.webhookUrl') : t('finish.url')}</span>
            <div class="secret">
              <input class="input" type={shown[a.id] ? 'text' : 'password'} value={a.url} placeholder={placeholder(a.kind)} onchange={text(a, 'url')} spellcheck="false" autocomplete="off" />
              <button class="btn ghost sm icon" onclick={() => (shown[a.id] = !shown[a.id])} aria-label={t('common.show')}><Icon name={shown[a.id] ? 'eye-off' : 'eye'} size={14} /></button>
            </div>
          </label>
          {#if a.kind === 'webhook'}<p class="hint">{t('finish.webhookHint')}</p>{/if}
        {:else if a.kind === 'telegram'}
          <label class="f wide">
            <span>{t('finish.botToken')}</span>
            <div class="secret">
              <input class="input" type={shown[a.id] ? 'text' : 'password'} value={a.token} placeholder="123456:ABC-DEF…" onchange={text(a, 'token')} spellcheck="false" autocomplete="off" />
              <button class="btn ghost sm icon" onclick={() => (shown[a.id] = !shown[a.id])} aria-label={t('common.show')}><Icon name={shown[a.id] ? 'eye-off' : 'eye'} size={14} /></button>
            </div>
          </label>
          <label class="f">
            <span>{t('finish.chatId')}</span>
            <input class="input" value={a.chatId} placeholder="-1001234567890" onchange={text(a, 'chatId')} spellcheck="false" />
          </label>
        {:else if a.kind === 'ntfy'}
          <label class="f">
            <span>{t('finish.server')}</span>
            <input class="input" value={a.url} placeholder="https://ntfy.sh" onchange={text(a, 'url')} spellcheck="false" />
          </label>
          <label class="f">
            <span>{t('finish.topic')}</span>
            <input class="input" value={a.topic} placeholder="nebula-…" onchange={text(a, 'topic')} spellcheck="false" />
          </label>
          <label class="f wide">
            <span>{t('finish.accessToken')} <em>({t('common.optional')})</em></span>
            <div class="secret">
              <input class="input" type={shown[a.id] ? 'text' : 'password'} value={a.token} placeholder="tk_…" onchange={text(a, 'token')} spellcheck="false" autocomplete="off" />
              <button class="btn ghost sm icon" onclick={() => (shown[a.id] = !shown[a.id])} aria-label={t('common.show')}><Icon name={shown[a.id] ? 'eye-off' : 'eye'} size={14} /></button>
            </div>
          </label>
        {:else if a.kind === 'script'}
          <label class="f wide">
            <span>{t('finish.script')}</span>
            <div class="secret">
              <input class="input" value={a.path} placeholder={isWindows ? 'C:\\scripts\\fin.bat' : '/home/…/fin.sh'} onchange={text(a, 'path')} spellcheck="false" />
              <button class="btn sm" onclick={() => pickScript(a)}>{t('common.browse')}</button>
            </div>
          </label>
          <label class="f small">
            <span>{t('finish.timeout')}</span>
            <div class="unit-wrap">
              <input class="input" type="number" min="1" max="3600" value={a.timeoutSecs}
                onchange={(e) => { const v = Math.max(1, Math.min(3600, Math.round(Number(e.target.value)) || 120)); e.target.value = v; update(a.id, { timeoutSecs: v }); }} />
              <span class="unit">s</span>
            </div>
          </label>
          <p class="hint">{t('finish.scriptEnv')} <code>NEBULA_NAME</code>, <code>NEBULA_STATUS</code>, <code>NEBULA_PATH</code>, <code>NEBULA_SIZE</code>, <code>NEBULA_ERROR</code>, <code>NEBULA_STAGE</code></p>
        {:else if power}
          <div class="f wide">
            <span>{t('finish.powerMode')}</span>
            <div class="seg">
              {#each ['shutdown', 'sleep', 'hibernate'] as m}
                <button class:sel={(a.power || 'shutdown') === m} onclick={() => update(a.id, { power: m })}><Icon name={m === 'shutdown' ? 'power' : 'moon'} size={13} /> {t(`finish.power.${m}`)}</button>
              {/each}
            </div>
          </div>
          <div class="f wide once">
            <div>
              <div class="o-label">{t('finish.once')}</div>
              <div class="hint">{t('finish.onceDesc')}</div>
            </div>
            <Switch checked={a.once} label={t('finish.once')} onchange={(v) => update(a.id, { once: v })} />
          </div>
          <p class="hint">{t('finish.powerNote')}</p>
        {/if}
      </div>
    </div>
  {/each}

  <div class="foot">
    <div class="add-wrap">
      <button class="btn sm primary" onclick={(e) => { e.stopPropagation(); adding = !adding; }}><Icon name="plus" size={14} /> {t('finish.add')}</button>
      {#if adding}
        <div class="menu card" transition:scale={{ start: 0.95, duration: d(150) }} onclick={(e) => e.stopPropagation()} role="presentation">
          {#each KINDS as k}
            <button class="m-item" onclick={() => add(k)}>
              <span class="k-icon sm k-{k}"><Icon name={ICONS[k]} size={14} /></span>
              <span class="m-text"><b>{t(`finish.kind.${k}`)}</b><small>{t(`finish.kindDesc.${k}`)}</small></span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
    <span class="lock"><Icon name="lock" size={12} /> {t('finish.encrypted')}</span>
  </div>
</div>

<style>
  .fa { display: flex; flex-direction: column; gap: 12px; }
  .fa.miss { display: none; }
  .intro { margin: 0; font-size: 12.5px; color: var(--muted); line-height: 1.5; }
  .act { padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; transition: opacity 0.2s; }
  .act.off .when, .act.off .fields { opacity: 0.55; }
  .head { display: flex; align-items: center; gap: 8px; }
  .k-icon { width: 32px; height: 32px; border-radius: 9px; display: grid; place-items: center; flex-shrink: 0; background: var(--panel-3); color: var(--accent-2); }
  .k-icon.sm { width: 26px; height: 26px; border-radius: 7px; }
  .k-discord { color: #8b9cff; background: rgba(88, 101, 242, 0.16); }
  .k-telegram { color: #5ec4f5; background: rgba(42, 171, 238, 0.15); }
  .k-ntfy { color: #6ee7b7; background: rgba(16, 185, 129, 0.15); }
  .k-script { color: #fcd34d; background: rgba(251, 191, 36, 0.13); }
  .k-power { color: #fca5a5; background: rgba(248, 113, 113, 0.14); }
  .name { flex: 1; min-width: 0; height: 32px; font-weight: 600; }
  .icon { width: 32px; padding: 0; justify-content: center; }
  .when { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .w-label { font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); font-weight: 650; margin-right: 4px; }
  .chip {
    display: inline-flex; align-items: center; gap: 5px; height: 26px; padding: 0 10px;
    border-radius: 999px; border: 1px solid var(--border-2); background: var(--panel-2);
    color: var(--text-2); font-size: 12px; font-weight: 600; cursor: pointer; transition: all 0.15s;
  }
  .chip :global(svg) { opacity: 0.5; }
  .chip.on { background: rgba(var(--accent-rgb), 0.16); border-color: rgba(var(--accent-rgb), 0.45); color: var(--text); }
  .chip.on :global(svg) { opacity: 1; color: var(--accent-2); }
  .chip.fixed { cursor: default; }
  .fields { display: flex; flex-wrap: wrap; gap: 10px; }
  .f { display: flex; flex-direction: column; gap: 4px; flex: 1 1 200px; min-width: 0; }
  .f.wide { flex-basis: 100%; }
  .f.small { flex: 0 0 120px; }
  .f > span { font-size: 12px; color: var(--muted); font-weight: 600; }
  .f em { font-style: normal; font-weight: 500; opacity: 0.8; }
  .f .input { height: 32px; font-size: 12.5px; min-width: 0; }
  .secret { display: flex; gap: 6px; }
  .secret .input { flex: 1; font-family: var(--mono); }
  .unit-wrap { display: flex; align-items: center; gap: 6px; }
  .unit-wrap .input { width: 80px; text-align: right; }
  .unit { font-size: 12.5px; color: var(--muted); }
  .hint { margin: 0; font-size: 12px; color: var(--muted); line-height: 1.5; flex-basis: 100%; }
  .hint code { font-family: var(--mono); font-size: 11px; padding: 1px 5px; border-radius: 5px; background: var(--panel-3); }
  .once { flex-direction: row; align-items: center; justify-content: space-between; gap: 12px; }
  .o-label { font-weight: 560; font-size: 13px; }
  .seg { display: inline-flex; align-self: flex-start; padding: 3px; gap: 3px; border-radius: 10px; background: var(--panel-2); border: 1px solid var(--border); flex-wrap: wrap; }
  .seg button {
    display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 12px; border: none; border-radius: 7px;
    background: transparent; color: var(--text-2); font-size: 12.5px; font-weight: 600; cursor: pointer;
  }
  .seg button.sel { background: rgba(var(--accent-rgb), 0.2); color: var(--text); box-shadow: inset 0 0 0 1px rgba(var(--accent-rgb), 0.45); }
  .foot { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .lock { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--muted); }
  .add-wrap { position: relative; }
  .menu {
    position: absolute; left: 0; bottom: 38px; z-index: 30; width: 280px; padding: 6px;
    background: var(--panel-2); box-shadow: var(--shadow); transform-origin: bottom left;
  }
  .m-item { width: 100%; display: flex; align-items: center; gap: 10px; padding: 7px 8px; border: none; border-radius: 8px; background: transparent; cursor: pointer; text-align: left; color: var(--text); }
  .m-item:hover { background: var(--panel-3); }
  .m-text { display: flex; flex-direction: column; min-width: 0; }
  .m-text b { font-size: 13px; font-weight: 600; }
  .m-text small { font-size: 11.5px; color: var(--muted); }
  @media (max-width: 720px) {
    .head { flex-wrap: wrap; }
    .name { flex-basis: calc(100% - 90px); }
    .menu { width: min(300px, calc(100vw - 40px)); }
  }
</style>
