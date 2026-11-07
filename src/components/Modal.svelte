<script>
  import { t } from '../lib/i18n.svelte.js';
  import { fade, scale } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Icon from './Icon.svelte';
  import { d } from '../lib/state.svelte.js';
  import { isMobile } from '../lib/platform.js';

  let {
    title = '',
    subtitle = '',
    icon = null,
    iconKind = 'accent',
    width = 560,
    onclose = null,
    dismissable = true,
    children,
    footer,
  } = $props();

  // En el móvil el diálogo sube desde abajo como una hoja; en el escritorio aparece en el centro.
  function sheetOrScale(node) {
    if (!isMobile) return scale(node, { start: 0.94, duration: d(240), easing: cubicOut });
    return { duration: d(280), easing: cubicOut, css: (t) => `transform: translateY(${(1 - t) * 100}%)` };
  }

  function onkey(e) {
    if (e.key === 'Escape' && dismissable && onclose) {
      e.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={onkey} />

<div
  class="backdrop"
  transition:fade={{ duration: d(180) }}
  onmousedown={(e) => {
    if (e.target === e.currentTarget && dismissable && onclose) onclose();
  }}
  role="presentation">
  <div
    class="modal"
    style={isMobile ? '' : `width: min(${width}px, calc(100vw - 48px))`}
    transition:sheetOrScale
    role="dialog"
    aria-modal="true">
    {#if title}
      <header>
        {#if icon}
          <div class="m-icon {iconKind}"><Icon name={icon} size={20} /></div>
        {/if}
        <div class="titles">
          <h2>{title}</h2>
          {#if subtitle}<p>{subtitle}</p>{/if}
        </div>
        {#if dismissable && onclose}
          <button class="icon-btn" onclick={onclose} aria-label={t('common.close')}><Icon name="x" size={18} /></button>
        {/if}
      </header>
    {/if}
    <div class="body">
      {@render children?.()}
    </div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: var(--titlebar-h) 0 0 0;
    background: rgba(5, 6, 10, 0.62);
    backdrop-filter: blur(6px);
    display: grid;
    place-items: center;
    z-index: 100;
    padding: 24px;
  }
  .modal {
    max-height: calc(100vh - 100px);
    display: flex;
    flex-direction: column;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.025), transparent 120px), var(--panel);
    border: 1px solid var(--border-2);
    border-radius: 18px;
    box-shadow: var(--shadow), 0 0 0 1px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 20px 20px 4px 22px;
  }
  .m-icon {
    width: 40px; height: 40px;
    border-radius: 12px;
    display: grid; place-items: center;
    flex-shrink: 0;
    background: rgba(var(--accent-rgb), 0.15);
    color: var(--accent-2);
  }
  .m-icon.err { background: rgba(248, 113, 113, 0.14); color: var(--err); }
  .m-icon.warn { background: rgba(251, 191, 36, 0.14); color: var(--warn); }
  .m-icon.ok { background: rgba(52, 211, 153, 0.14); color: var(--ok); }
  .titles { flex: 1; min-width: 0; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 17px; font-weight: 650; line-height: 1.3; word-break: break-word; }
  header p { margin: 3px 0 0; color: var(--text-2); font-size: 13px; }
  .body { padding: 16px 22px 20px; overflow-y: auto; }
  :global(html.mobile) .backdrop { padding: 0; place-items: end stretch; }
  :global(html.mobile) .modal {
    width: 100%;
    max-height: calc(100% - 24px);
    border-radius: 20px 20px 0 0;
    border-bottom: none;
  }
  :global(html.mobile) header { padding: 18px 16px 4px 18px; }
  :global(html.mobile) .body { padding: 14px 16px 18px; }
  :global(html.mobile) footer { padding: 12px 16px; flex-wrap: wrap; }
  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px 22px;
    border-top: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.15);
  }
</style>
