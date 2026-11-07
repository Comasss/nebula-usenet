<script>
  // Menú "Descargar con": se abre al mantener pulsado Descargar y deja elegir todos los
  // servidores NNTP, uno en concreto o un servicio debrid. En el móvil sale como hoja de cristal.
  import { t } from '../lib/i18n.svelte.js';
  import { fade, fly, scale } from 'svelte/transition';
  import { cubicOut, backOut } from 'svelte/easing';
  import Icon from './Icon.svelte';
  import { ui, d } from '../lib/state.svelte.js';
  import { isMobile } from '../lib/platform.js';
  import { haptic } from '../lib/feedback.js';

  const m = $derived(ui.viaMenu);
  const sheet = isMobile || window.matchMedia('(max-width: 700px)').matches;

  const pos = $derived.by(() => {
    const r = m?.rect;
    if (!r || sheet) return null;
    const w = 280;
    const h = Math.min(420, 70 + (m.options.length + 1) * 54);
    const left = Math.max(12, Math.min(window.innerWidth - w - 12, r.right - w));
    const up = window.innerHeight - r.bottom < h + 16;
    return { left, top: up ? Math.max(12, r.top - h - 8) : r.bottom + 8, w, up };
  });

  function pick(v) {
    haptic('success');
    m?.resolve(v);
  }

  function onkey(e) {
    if (m && e.key === 'Escape') {
      e.stopPropagation();
      m.resolve(null);
    }
  }
</script>

<svelte:window onkeydown={onkey} />

{#snippet list()}
  <div class="v-head">
    <span class="v-ic"><Icon name="download" size={16} stroke={2.4} /></span>
    <div class="v-titles">
      <strong>{t('via.title')}</strong>
      {#if m.title}<span class="truncate">{m.title}</span>{/if}
    </div>
  </div>
  <div class="v-items">
    {#each m.options as o, i (o.value)}
      {#if o.section && o.section !== m.options[i - 1]?.section}
        <div class="v-sec"><Icon name={o.sectionIcon ?? 'download'} size={12} /> {o.section}</div>
      {:else if i > 0 && o.group !== m.options[i - 1].group}<div class="v-sep"></div>{/if}
      <button
        type="button"
        class="v-opt"
        class:child={o.child}
        class:sel={o.value === m.current}
        style="--i:{i}"
        onclick={() => pick(o.value)}>
        <span class="o-ic {o.group}"><Icon name={o.child ? 'chevron-right' : o.icon} size={o.child ? 13 : 16} /></span>
        <span class="o-text">
          <strong class="truncate">{o.label}</strong>
          <small class="truncate">{o.sub}</small>
        </span>
        {#if o.value === m.current}<Icon name="check" size={15} />{/if}
      </button>
    {/each}
  </div>
{/snippet}

{#if m}
  {#if sheet}
    <div class="backdrop" transition:fade={{ duration: d(160) }} onclick={() => m.resolve(null)} role="presentation"></div>
    <div class="sheet glass" role="dialog" transition:fly={{ y: 320, duration: d(320), easing: cubicOut }}>
      <div class="grip"></div>
      {@render list()}
    </div>
  {:else}
    <div class="backdrop clear" onclick={() => m.resolve(null)} role="presentation"></div>
    <div class="pop glass" role="dialog" style="left:{pos?.left ?? 0}px; top:{pos?.top ?? 0}px; width:{pos?.w ?? 280}px"
      transition:scale={{ start: 0.9, duration: d(220), easing: backOut }}>
      {@render list()}
    </div>
  {/if}
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 300; background: rgba(5, 6, 10, 0.5); backdrop-filter: blur(3px); }
  .backdrop.clear { background: transparent; backdrop-filter: none; }
  .sheet {
    position: fixed;
    left: 10px; right: 10px;
    bottom: calc(10px + env(safe-area-inset-bottom, 0px));
    z-index: 301;
    max-height: 75vh;
    overflow-y: auto;
    padding: 8px 10px 12px;
    border-radius: 28px;
  }
  .pop { position: fixed; z-index: 301; padding: 8px; border-radius: 18px; max-height: 420px; overflow-y: auto; }
  .grip { width: 40px; height: 4px; border-radius: 4px; background: rgba(255, 255, 255, 0.22); margin: 2px auto 10px; }
  .v-head { display: flex; align-items: center; gap: 10px; padding: 4px 8px 10px; }
  .v-ic { width: 32px; height: 32px; border-radius: 10px; display: grid; place-items: center; background: var(--grad-diag); color: #fff; flex-shrink: 0; box-shadow: 0 6px 16px -6px rgba(var(--accent-rgb), 0.9); }
  .v-titles { display: flex; flex-direction: column; min-width: 0; }
  .v-titles strong { font-size: 14.5px; }
  .v-titles span { font-size: 12px; color: var(--muted); }
  .v-items { display: flex; flex-direction: column; gap: 2px; }
  .v-sec { display: flex; align-items: center; gap: 6px; margin: 10px 10px 4px; font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.07em; color: var(--muted); }
  .v-sec:first-child { margin-top: 2px; }
  .v-sep { height: 1px; background: rgba(255, 255, 255, 0.07); margin: 6px 8px; }
  .v-opt {
    display: flex; align-items: center; gap: 12px;
    width: 100%; min-height: 54px; padding: 8px 10px;
    border: none; border-radius: 14px;
    background: transparent; color: var(--text);
    text-align: left; cursor: pointer;
    transition: background 0.18s, transform 0.15s var(--ease);
    animation: rise 0.34s var(--ease) backwards;
    animation-delay: calc(var(--i) * 35ms);
  }
  .v-opt:hover { background: rgba(255, 255, 255, 0.06); }
  .v-opt:active { transform: scale(0.98); background: rgba(255, 255, 255, 0.09); }
  .v-opt.sel { background: rgba(var(--accent-rgb), 0.16); }
  .v-opt.child { padding-left: 26px; min-height: 48px; }
  .v-opt > :global(svg) { color: var(--accent-2); flex-shrink: 0; }
  @keyframes rise { from { opacity: 0; transform: translateY(8px); } }
  .o-ic { width: 34px; height: 34px; border-radius: 11px; display: grid; place-items: center; flex-shrink: 0; background: rgba(var(--accent-rgb), 0.16); color: var(--accent-2); }
  .o-ic.debrid { background: rgba(96, 165, 250, 0.16); color: #93c5fd; }
  .child .o-ic { width: 26px; height: 26px; border-radius: 8px; background: rgba(255, 255, 255, 0.05); color: var(--muted); }
  .o-text { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .o-text strong { font-size: 14px; font-weight: 620; }
  .o-text small { font-size: 12px; color: var(--muted); }
</style>
