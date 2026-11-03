import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import PowerCountdown from './components/PowerCountdown.svelte';
import { isAndroid } from './lib/platform.js';

if (isAndroid) document.documentElement.classList.add('mobile');

// Menú contextual del navegador desactivado salvo en campos de texto
document.addEventListener('contextmenu', (e) => {
  if (!e.target.closest('input, textarea, .selectable')) e.preventDefault();
});

// La ventana de la cuenta atrás de apagado carga la misma página con #power.
const Root = location.hash === '#power' ? PowerCountdown : App;
export default mount(Root, { target: document.getElementById('app') });
