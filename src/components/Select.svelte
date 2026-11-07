<script>
  // Desplegable con el estilo de Nebula (el <select> nativo abre la lista del sistema, con
  // otro aspecto en cada plataforma). En el móvil la lista sale como hoja desde abajo.
  import { fade, fly, scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Icon from './Icon.svelte';
  import { d } from '../lib/state.svelte.js';
  import { isMobile } from '../lib/platform.js';

  let { value = $bindable(), options = [], onchange = null, class: cls = '', title = '', id = undefined, label = '' } = $props();

  let open = $state(false);
  let btn;
  let pos = $state({ left: 0, top: 0, width: 0, up: false, max: 320 });
  const sheet = isMobile || window.matchMedia('(max-width: 700px)').matches;
  const current = $derived(options.find((o) => o.value === value));

  function toggle() {
    if (open) return (open = false);
    const r = btn.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 12;
    const above = r.top - 12;
    const up = below < 220 && above > below;
    pos = { left: r.left, top: up ? r.top - 6 : r.bottom + 6, width: Math.max(r.width, 180), up, max: Math.min(340, up ? above : below) };
    open = true;
  }

  function pick(o) {
    open = false;
    if (o.value === value) return;
    value = o.value;
    onchange?.(o.value);
  }

  function onkey(e) {
    if (open && e.key === 'Escape') {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<svelte:window onkeydown={onkey} onresize={() => (open = false)} />

<button bind:this={btn} type="button" class="select sel-btn {cls}" class:open {id} {title} aria-haspopup="listbox" aria-expanded={open} onclick={toggle}>
  <span class="truncate">{current?.label ?? ''}</span>
</button>

{#if open}
  {#if sheet}
    <div class="backdrop" transition:fade={{ duration: d(160) }} onclick={() => (open = false)} role="presentation"></div>
    <div class="sheet" role="listbox" transition:fly={{ y: 300, duration: d(260), easing: cubicOut }}>
      <div class="grip"></div>
      {#if label || title}<div class="s-title">{label || title}</div>{/if}
      <div class="items">
        {#each options as o (o.value)}
          <button type="button" class="opt" class:sel={o.value === value} onclick={() => pick(o)} role="option" aria-selected={o.value === value}>
            <span class="truncate">{o.label}</span>
            {#if o.value === value}<Icon name="check" size={16} />{/if}
          </button>
        {/each}
      </div>
    </div>
  {:else}
    <div class="catcher" onmousedown={() => (open = false)} role="presentation"></div>
    <div
      class="pop card"
      class:up={pos.up}
      role="listbox"
      style="left:{pos.left}px; {pos.up ? `bottom:${window.innerHeight - pos.top}px` : `top:${pos.top}px`}; min-width:{pos.width}px; max-height:{pos.max}px"
      transition:scale={{ start: 0.96, duration: d(150), easing: cubicOut }}>
      {#each options as o (o.value)}
        <button type="button" class="opt" class:sel={o.value === value} onclick={() => pick(o)} role="option" aria-selected={o.value === value}>
          <span class="truncate">{o.label}</span>
          {#if o.value === value}<Icon name="check" size={14} />{/if}
        </button>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .sel-btn { display: flex; align-items: center; text-align: left; min-width: 0; }
  .sel-btn.open { border-color: rgba(var(--accent-rgb), 0.85); box-shadow: 0 0 0 3px rgba(var(--accent-rgb), 0.18); }
  .catcher { position: fixed; inset: 0; z-index: 250; }
  .pop {
    position: fixed; z-index: 251;
    padding: 6px; overflow-y: auto;
    background: var(--panel-2);
    box-shadow: var(--shadow);
    transform-origin: top left;
  }
  .pop.up { transform-origin: bottom left; }
  .opt {
    width: 100%; min-height: 32px;
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 0 10px; border: none; border-radius: 7px;
    background: transparent; color: var(--text); cursor: pointer; font-size: 13px; text-align: left;
  }
  .opt:hover { background: var(--panel-3); }
  .opt.sel { color: var(--accent-2); font-weight: 600; }
  .backdrop { position: fixed; inset: 0; z-index: 250; background: rgba(5, 6, 10, 0.62); backdrop-filter: blur(4px); }
  .sheet {
    position: fixed; left: 0; right: 0; bottom: 0; z-index: 251;
    max-height: 70vh; display: flex; flex-direction: column;
    padding: 8px 10px calc(14px + env(safe-area-inset-bottom, 0px));
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.025), transparent 120px), var(--panel);
    border: 1px solid var(--border-2); border-bottom: none;
    border-radius: 20px 20px 0 0;
    box-shadow: var(--shadow);
  }
  .grip { width: 38px; height: 4px; border-radius: 4px; background: var(--panel-4); margin: 2px auto 10px; flex-shrink: 0; }
  .s-title { font-size: 12px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); font-weight: 650; padding: 0 10px 8px; }
  .items { overflow-y: auto; }
  .sheet .opt { min-height: 48px; font-size: 15px; border-radius: 12px; padding: 0 14px; }
</style>
