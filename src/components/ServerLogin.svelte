<script>
  // nebula-server en el navegador: crear el usuario la primera vez o entrar. Se muestra antes
  // que la interfaz; al terminar se recarga la página ya con la sesión abierta.
  import { t } from '../lib/i18n.svelte.js';
  import Icon from './Icon.svelte';
  import logo from '../assets/logo.svg';

  let { status } = $props();
  const setup = !!status?.needsSetup;
  const min = status?.minPassword ?? 8;
  let username = $state(setup ? 'admin' : '');
  let password = $state('');
  let repeat = $state('');
  let busy = $state(false);
  let error = $state(status ? '' : t('web.unreachable'));

  async function submit(e) {
    e.preventDefault();
    error = '';
    if (setup && password !== repeat) {
      error = t('web.mismatch');
      return;
    }
    busy = true;
    try {
      const res = await fetch(setup ? 'api/v1/auth/setup' : 'api/v1/auth/login', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        credentials: 'same-origin',
        body: JSON.stringify({ username, password }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) throw data.error ?? `HTTP ${res.status}`;
      location.reload();
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }
</script>

<div class="screen">
  <form class="box card" onsubmit={submit}>
    <div class="brand">
      <img src={logo} alt="" width="44" height="44" />
      <div>
        <div class="name grad-text">Nebula</div>
        <div class="muted small">Nebula Server{status?.version ? ` ${status.version}` : ''}</div>
      </div>
    </div>
    <h1>{setup ? t('web.setupTitle') : t('web.loginTitle')}</h1>
    {#if setup}<p class="muted">{t('web.setupDesc', { n: min })}</p>{/if}
    <label>
      <span>{t('web.username')}</span>
      <input class="input" bind:value={username} autocomplete="username" autocapitalize="off" spellcheck="false" required />
    </label>
    <label>
      <span>{t('web.password')}</span>
      <input class="input" type="password" bind:value={password} autocomplete={setup ? 'new-password' : 'current-password'} minlength={setup ? min : undefined} required />
    </label>
    {#if setup}
      <label>
        <span>{t('web.repeat')}</span>
        <input class="input" type="password" bind:value={repeat} autocomplete="new-password" required />
      </label>
    {/if}
    {#if error}<div class="err"><Icon name="alert-triangle" size={14} /> {error}</div>{/if}
    <button class="btn primary" type="submit" disabled={busy || !username.trim() || !password}>
      {#if busy}<Icon name="loader" size={16} class="spin" />{/if}
      {setup ? t('web.create') : t('web.signIn')}
    </button>
  </form>
</div>

<style>
  .screen { min-height: 100vh; display: grid; place-items: center; padding: 16px; background: var(--bg); }
  .box { width: min(400px, 100%); padding: 26px 24px; display: flex; flex-direction: column; gap: 14px; }
  .brand { display: flex; align-items: center; gap: 12px; }
  .name { font-family: var(--font-display); font-size: 22px; font-weight: 700; }
  .small { font-size: 12px; }
  h1 { margin: 6px 0 0; font-family: var(--font-display); font-size: 20px; font-weight: 650; }
  p { margin: 0; font-size: 13px; line-height: 1.5; }
  label { display: flex; flex-direction: column; gap: 6px; }
  label span { font-size: 12.5px; color: var(--muted); font-weight: 600; }
  .input { height: 40px; }
  .err { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--err); }
  .btn { height: 42px; justify-content: center; }
</style>
