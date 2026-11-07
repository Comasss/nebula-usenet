<script>
  // Ajustes del control remoto.
  // - PC: activarlo, ver la dirección y los móviles emparejados (y revocarlos).
  // - Android ("Controlar un PC"): buscar PCs en la red, emparejarlos, añadir uno a mano y controlarlo.
  import { t } from '../lib/i18n.svelte.js';
  import { invoke, listen } from '../lib/transport.js';
    import Icon from './Icon.svelte';
  import SettingRow from './SettingRow.svelte';
  import Modal from './Modal.svelte';
  import { ui, saveSettings, ask } from '../lib/state.svelte.js';
  import { isAndroid } from '../lib/platform.js';
  import { remote, refreshPcs, searchPcs, pairPc, cancelPairing, forgetPc, controlPc, stopControl } from '../lib/remote.svelte.js';
  import * as f from '../lib/format.js';

  const s = $derived(ui.settings);

  // ---- PC ----
  let status = $state(null);
  let devices = $state([]);

  $effect(() => {
    if (isAndroid) return;
    const offs = [];
    invoke('remote_status').then((st) => (status = st)).catch(() => {});
    invoke('remote_devices').then((d) => (devices = d)).catch(() => {});
    listen('remote-status', (e) => (status = e.payload)).then((fn) => offs.push(fn));
    listen('remote-devices', (e) => (devices = e.payload)).then((fn) => offs.push(fn));
    return () => offs.forEach((fn) => fn());
  });

  async function revoke(dev) {
    const r = await ask({ title: t('remote.revokeTitle', { name: dev.name }), message: t('remote.revokeText'), confirmLabel: t('remote.revoke'), danger: true, icon: 'trash' });
    if (r.ok) await invoke('remote_revoke', { id: dev.id });
  }

  function setPort(e) {
    let v = Math.round(Number(e.target.value));
    if (!v || v < 1024 || v > 65535) v = 5078;
    e.target.value = v;
    saveSettings({ remotePort: v });
  }

  // ---- Android ----
  let manual = $state('');
  let searched = $state(false);

  $effect(() => {
    if (!isAndroid) return;
    refreshPcs();
    search();
  });

  async function search() {
    await searchPcs();
    searched = true;
  }

  async function pairAndControl(addr) {
    const pc = await pairPc(addr);
    if (pc) controlPc(pc);
  }

  async function addManual() {
    const a = manual.trim();
    if (!a) return;
    const pc = await pairPc(a);
    if (pc) {
      manual = '';
      controlPc(pc);
    }
  }

  const unpaired = $derived(remote.found.filter((x) => !remote.pcs.some((p) => p.id === x.id)));
  const code = $derived(remote.pairing?.code ? `${remote.pairing.code.slice(0, 3)} ${remote.pairing.code.slice(3)}` : '');
</script>

{#if s}
  {#if !isAndroid}
    <div class="card group">
      <SettingRow icon="smartphone" label={t('remote.enable')} description={t('remote.enableDesc')} keywords={`${t('remote.phones')} Android`} checked={!!s.remoteEnabled} onchange={(v) => saveSettings({ remoteEnabled: v })} />
      {#if s.remoteEnabled}
        {#if status?.error}
          <div class="note err"><Icon name="alert-triangle" size={14} /> {status.error}</div>
        {:else if status?.warning}
          <div class="note warn"><Icon name="alert-triangle" size={14} /> {status.warning}</div>
        {/if}
        <SettingRow icon="wifi" label={t('remote.thisPc', { name: status?.name ?? '' })} description={t('remote.addressDesc')}>
          <div class="urls">
            {#if status?.running}
              {#each status.addresses as a}<code>{a}</code>{:else}<code>:{status.port}</code>{/each}
            {:else}
              <span class="chip">{t('api.stopped')}</span>
            {/if}
          </div>
        </SettingRow>
        <SettingRow label={t('remote.port')} description={t('remote.portDesc')}>
          <input class="input small" type="number" min="1024" max="65535" value={s.remotePort || 5078} onchange={setPort} />
        </SettingRow>
        <div class="note"><Icon name="shield" size={14} /> {t('remote.firewallNote')}</div>
      {/if}
    </div>

    <h4>{t('remote.phones')}</h4>
    <div class="card group">
      {#each devices as dev (dev.id)}
        <SettingRow icon="smartphone" label={dev.name} description={dev.online ? t('remote.connectedNow') : t('remote.pairedOn', { date: f.dateTime(dev.pairedAt), seen: f.ago(dev.lastSeen) })}>
          {#if dev.online}<span class="chip ok"><span class="dot"></span> {t('remote.online')}</span>{/if}
          <button class="btn sm danger" onclick={() => revoke(dev)}><Icon name="x" size={13} /> {t('remote.revoke')}</button>
        </SettingRow>
      {:else}
        <div class="empty-row"><Icon name="smartphone" size={16} /> {t('remote.noPhones')}</div>
      {/each}
    </div>
  {:else}
    <div class="card group">
      <div class="intro"><Icon name="monitor" size={16} /> {t('remote.intro')}</div>
      {#each remote.pcs as pc (pc.id)}
        {@const on = remote.active?.id === pc.id}
        <SettingRow icon="monitor" label={pc.name} description={on ? t('remote.controlling', { name: pc.name }) : pc.addr}>
          <button class="btn sm ghost icon" onclick={() => forgetPc(pc)} aria-label={t('remote.forget')} title={t('remote.forget')}><Icon name="trash" size={14} /></button>
          {#if on}
            <button class="btn sm" onclick={stopControl}>{t('remote.backToPhone')}</button>
          {:else}
            <button class="btn sm primary" onclick={() => controlPc(pc)}>{t('remote.control')}</button>
          {/if}
        </SettingRow>
      {/each}
    </div>

    <h4 class="with-btn">
      <span>{t('remote.foundTitle')}</span>
      <button class="btn sm ghost" onclick={search} disabled={remote.searching}>
        <span class="spin-wrap" class:spin={remote.searching}><Icon name="refresh" size={13} /></span> {remote.searching ? t('remote.searching') : t('remote.searchAgain')}
      </button>
    </h4>
    <div class="card group">
      {#each unpaired as pc (pc.id)}
        <SettingRow icon="monitor" label={pc.name} description={`${pc.addr} · Nebula ${pc.version}`}>
          <button class="btn sm primary" onclick={() => pairAndControl(pc.addr)} disabled={!!remote.pairing}>{t('remote.pair')}</button>
        </SettingRow>
      {:else}
        <div class="empty-row">
          {#if remote.searching || !searched}
            <span class="spin-wrap spin"><Icon name="refresh" size={15} /></span> {t('remote.searching')}
          {:else}
            <Icon name="info" size={15} /> {remote.found.length ? t('remote.allPaired') : t('remote.noneFound')}
          {/if}
        </div>
      {/each}
    </div>

    <h4>{t('remote.manualTitle')}</h4>
    <div class="card group">
      <form class="manual" onsubmit={(e) => { e.preventDefault(); addManual(); }}>
        <input class="input" bind:value={manual} placeholder="192.168.1.20:5078" inputmode="url" autocapitalize="off" spellcheck="false" />
        <button class="btn sm primary" type="submit" disabled={!manual.trim() || !!remote.pairing}>{t('remote.add')}</button>
      </form>
      <div class="note">{t('remote.manualDesc')}</div>
    </div>

    {#if remote.pairing}
      <Modal title={t('remote.pairingTitle')} icon="smartphone" width={420} onclose={cancelPairing}>
        {#if code}
          <p class="msg">{t('remote.pairingCheck', { name: remote.pairing.name })}</p>
          <div class="code">{code}</div>
        {:else}
          <p class="msg"><span class="spin-wrap spin"><Icon name="refresh" size={14} /></span> {t('remote.pairingConnecting', { address: remote.pairing.addr })}</p>
        {/if}
        {#snippet footer()}
          <button class="btn ghost" onclick={cancelPairing}>{t('common.cancel')}</button>
        {/snippet}
      </Modal>
    {/if}
  {/if}
{/if}

<style>
  .group { overflow: hidden; }
  h4 { margin: 18px 4px 8px; font-size: 12px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); }
  h4.with-btn { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  h4 .btn { text-transform: none; letter-spacing: 0; }
  .note { display: flex; align-items: center; gap: 8px; padding: 0 18px 12px 64px; margin-top: -4px; font-size: 12.5px; color: var(--muted); }
  .note.err { color: var(--err); }
  .note.warn { color: #fbbf24; }
  .urls { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; }
  .urls code { padding: 4px 8px; border-radius: 7px; background: var(--panel-3); border: 1px solid var(--border-2); user-select: all; }
  .small { width: 90px; height: 32px; text-align: right; }
  .icon { width: 32px; padding: 0; justify-content: center; }
  .empty-row, .intro { display: flex; align-items: center; gap: 10px; padding: 14px 18px; font-size: 13px; color: var(--muted); }
  .intro { color: var(--text-2); line-height: 1.45; }
  .intro + :global(.srow) { border-top: 1px solid var(--border); }
  .manual { display: flex; gap: 8px; padding: 12px 14px 10px; }
  .manual .input { flex: 1; min-width: 0; height: 36px; font-family: var(--mono); }
  .manual + .note { padding: 0 14px 12px; margin: 0; }
  .chip.ok { display: inline-flex; align-items: center; gap: 5px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: #34d399; }
  .msg { margin: 0 0 6px; color: var(--text-2); line-height: 1.5; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .code { margin: 14px 0 6px; text-align: center; font-family: var(--mono); font-size: 38px; font-weight: 700; letter-spacing: 0.12em; color: var(--text); }
  .spin-wrap { display: inline-flex; }
  .spin-wrap.spin { animation: spin 0.9s linear infinite; }
</style>
