import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import PowerCountdown from './components/PowerCountdown.svelte';
import ServerLogin from './components/ServerLogin.svelte';
import { isMobile, isServer } from './lib/platform.js';
import { session } from './lib/transport.js';

if (isMobile) document.documentElement.classList.add('mobile');
if (isServer) document.documentElement.classList.add('web');

// Menú contextual del navegador desactivado salvo en campos de texto (en la app).
if (!isServer) {
  document.addEventListener('contextmenu', (e) => {
    if (!e.target.closest('input, textarea, .selectable')) e.preventDefault();
  });
}

const target = document.getElementById('app');

async function start() {
  // En el navegador hay que entrar antes (o crear el usuario la primera vez).
  if (isServer) {
    const status = await fetch('api/v1/auth', { credentials: 'same-origin' })
      .then((r) => r.json())
      .catch(() => null);
    if (!status || status.needsSetup || !status.user) return mount(ServerLogin, { target, props: { status } });
    // Si la sesión caduca o se cierra en otra pestaña, se vuelve a pedir.
    session.onAuthNeeded = () => location.reload();
  }
  // La ventana de la cuenta atrás de apagado carga la misma página con #power.
  const Root = location.hash === '#power' ? PowerCountdown : App;
  return mount(Root, { target });
}

export default start();
