import { t } from './i18n.svelte.js';
import { defaultVia, usableDebrid } from './debrid.js';

// Categorías (tv, movies…). La Default (`*`) es fija y equivale a los ajustes globales; no está en
// settings.categories. La misma lógica vive en el motor (crates/nebula-core/src/categories.rs).

export const DEFAULT_CAT = '*';

export const PRIORITIES = [2, 1, 0, -1];
export const PP_LEVELS = [-1, 0, 1, 2, 3];

export function isDefault(name) {
  const n = String(name ?? '').trim().toLowerCase();
  return !n || n === '*' || n === 'default' || n === 'none';
}

/** Nombre que se muestra: «Default» traducido para `*`. */
export function catLabel(name) {
  return isDefault(name) ? t('cats.default') : name;
}

export function categoryList(settings) {
  return settings?.categories ?? [];
}

export function findCategory(settings, name) {
  if (isDefault(name)) return null;
  const n = String(name).trim().toLowerCase();
  return categoryList(settings).find((c) => c.name.toLowerCase() === n) ?? null;
}

/** Opciones para un Select: Default primero y luego las propias. */
export function categoryOptions(settings) {
  return [{ value: DEFAULT_CAT, label: t('cats.default') }, ...categoryList(settings).map((c) => ({ value: c.name, label: c.name }))];
}

export function priorityLabel(p) {
  return t(`cats.prio.${p === 2 ? 'force' : p === 1 ? 'high' : p === -1 ? 'low' : 'normal'}`);
}

export function priorityOptions() {
  return PRIORITIES.map((p) => ({ value: p, label: priorityLabel(p) }));
}

export function ppLabel(pp) {
  return t(`cats.pp.${pp < 0 ? 'global' : pp}`);
}

export function parseRanges(spec) {
  return String(spec ?? '')
    .replace(/\s*-\s*/g, '-')
    .split(/[,; ]+/)
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => {
      const [a, b] = p.split('-').map((x) => parseInt(x, 10));
      if (isNaN(a) || (p.includes('-') && isNaN(b))) return null;
      const hi = p.includes('-') ? b : a;
      return [Math.min(a, hi), Math.max(a, hi)];
    })
    .filter(Boolean);
}

/** Categoría propia para las categorías Newznab de un resultado (gana el rango más estrecho). */
export function categoryForNewznab(settings, ids = []) {
  let best = null;
  for (const c of categoryList(settings)) {
    for (const [a, b] of parseRanges(c.newznab)) {
      if (ids.some((id) => id >= a && id <= b) && (!best || b - a < best.w)) best = { w: b - a, name: c.name };
    }
  }
  return best?.name ?? DEFAULT_CAT;
}

function isAbsolute(dir) {
  return /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(dir);
}

export function categoryDir(settings, cat) {
  const base = settings?.downloadDir ?? '';
  const dir = cat?.dir?.trim();
  if (!dir) return base;
  if (isAbsolute(dir)) return dir;
  const sep = base.includes('\\') && !base.includes('/') ? '\\' : '/';
  return `${base.replace(/[\\/]+$/, '')}${sep}${dir.replace(/^[\\/]+/, '')}`;
}

export function globalOptions(s) {
  return { par2: s.par2Enabled, extract: s.extractEnabled, deleteArchives: s.deleteArchives, deletePar2: s.deletePar2, createFolder: s.createJobFolder };
}

export function optionsForPp(s, pp) {
  const o = globalOptions(s);
  if (pp === 0) return { ...o, par2: false, extract: false, deleteArchives: false, deletePar2: false };
  if (pp === 1) return { ...o, par2: true, extract: false, deleteArchives: false, deletePar2: false };
  if (pp === 2) return { ...o, par2: true, extract: true, deleteArchives: false, deletePar2: false };
  if (pp === 3) return { ...o, par2: true, extract: true, deleteArchives: true, deletePar2: true };
  return o;
}

/** Método de una categoría tal como lo espera el diálogo de añadir ('debrid' = el primer servicio). */
function viaFor(settings, via) {
  if (!via) return defaultVia(settings);
  if (via === 'debrid') return usableDebrid(settings)[0]?.id ?? 'nntp';
  return via;
}

/** Valores por defecto de un trabajo de la categoría `name`: carpeta, método, prioridad y opciones. */
export function categoryDefaults(settings, name) {
  const cat = findCategory(settings, name);
  return {
    category: cat?.name ?? DEFAULT_CAT,
    dest: categoryDir(settings, cat),
    via: viaFor(settings, cat?.via),
    priority: cat?.priority ?? 0,
    options: optionsForPp(settings, cat?.pp ?? -1),
  };
}

export function blankCategory(name = '') {
  return { name, dir: '', via: '', priority: 0, pp: -1, newznab: '' };
}

/** Categorías de ejemplo con los rangos Newznab habituales. */
export function presetCategories() {
  return [
    { ...blankCategory('tv'), dir: 'tv', newznab: '5000-5999' },
    { ...blankCategory('movies'), dir: 'movies', newznab: '2000-2999' },
    { ...blankCategory('audio'), dir: 'audio', newznab: '3000-3999' },
    { ...blankCategory('books'), dir: 'books', newznab: '7000-7999' },
  ];
}
