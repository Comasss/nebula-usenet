<script>
  // Lista editable de textos cortos (contraseñas, extensiones…): una ficha por elemento y un
  // campo para añadir más.
  import { scale } from 'svelte/transition';
  import Icon from './Icon.svelte';
  import { d } from '../lib/state.svelte.js';

  let { items = [], onchange, placeholder = '', removeLabel = '', normalize = (v) => v.trim() } = $props();
  let value = $state('');

  function add() {
    const v = normalize(value);
    value = '';
    if (!v || items.includes(v)) return;
    onchange([...items, v]);
  }
</script>

<div class="chips">
  {#each items as p (p)}
    <span class="pchip" in:scale={{ start: 0.7, duration: d(200) }}>
      <code>{p}</code>
      <button onclick={() => onchange(items.filter((x) => x !== p))} aria-label={removeLabel} title={removeLabel}><Icon name="x" size={12} /></button>
    </span>
  {/each}
  <form class="p-add" onsubmit={(e) => { e.preventDefault(); add(); }}>
    <input class="input" {placeholder} bind:value spellcheck="false" autocomplete="off" onblur={add} />
  </form>
</div>

<style>
  .chips { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; align-items: center; }
  .pchip { display: inline-flex; align-items: center; gap: 4px; height: 28px; max-width: 100%; padding: 0 4px 0 10px; border-radius: 8px; background: var(--panel-3); border: 1px solid var(--border-2); }
  .pchip code { font-family: var(--mono); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pchip button { display: grid; place-items: center; flex: none; width: 20px; height: 20px; border: none; border-radius: 5px; background: none; color: var(--muted); cursor: pointer; }
  .pchip button:hover { background: rgba(248, 113, 113, 0.18); color: var(--err); }
  .p-add .input { height: 28px; width: 160px; font-size: 12.5px; }
</style>
