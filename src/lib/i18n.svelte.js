// Traducciones de la interfaz. Los textos están en src/locales/<código>.json; `es` es el original
// y `en` el idioma de reserva cuando falta una cadena o el idioma del sistema no está disponible.
import { invoke } from './transport.js';

const files = import.meta.glob('../locales/*.json', { eager: true, import: 'default' });

/** Idiomas disponibles, con su nombre en ese idioma y la configuración regional para fechas y números. */
export const LANGUAGES = [
  { code: 'en', name: 'English', locale: 'en-US' },
  { code: 'es', name: 'Español', locale: 'es-ES' },
  { code: 'zh', name: '简体中文', locale: 'zh-CN' },
  { code: 'hi', name: 'हिन्दी', locale: 'hi-IN' },
  { code: 'pt', name: 'Português (Brasil)', locale: 'pt-BR' },
  { code: 'fr', name: 'Français', locale: 'fr-FR' },
  { code: 'de', name: 'Deutsch', locale: 'de-DE' },
  { code: 'ru', name: 'Русский', locale: 'ru-RU' },
  { code: 'ja', name: '日本語', locale: 'ja-JP' },
  { code: 'ar', name: 'العربية', locale: 'ar-u-nu-latn' },
  { code: 'it', name: 'Italiano', locale: 'it-IT' },
  { code: 'ca', name: 'Català', locale: 'ca-ES' },
];
export const FALLBACK = 'en';

function flatten(obj, prefix = '', out = {}) {
  for (const [k, v] of Object.entries(obj ?? {})) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === 'object') flatten(v, key, out);
    else out[key] = String(v);
  }
  return out;
}

const DICTS = {};
for (const l of LANGUAGES) DICTS[l.code] = flatten(files[`../locales/${l.code}.json`]);

/** Idioma del sistema (el primero de la lista de preferencias que esté traducido) o inglés. */
export function systemLanguage() {
  const prefs = navigator.languages?.length ? navigator.languages : [navigator.language || FALLBACK];
  for (const p of prefs) {
    const base = String(p).toLowerCase().split(/[-_]/)[0];
    if (DICTS[base]) return base;
  }
  return FALLBACK;
}

export const i18n = $state({ lang: systemLanguage() });

/** Aplica la preferencia guardada ('' = automático, según el sistema). */
export function setLanguage(pref) {
  const lang = pref && DICTS[pref] ? pref : systemLanguage();
  i18n.lang = lang;
  document.documentElement.lang = lang;
  invoke('set_language', { lang }).catch(() => {});
  return lang;
}

/** Configuración regional (BCP 47) del idioma actual, para Intl y toLocale*. */
export function locale() {
  return LANGUAGES.find((l) => l.code === i18n.lang)?.locale ?? 'en-US';
}

function lookup(lang, key) {
  return DICTS[lang]?.[key] ?? DICTS[FALLBACK][key] ?? DICTS.es[key];
}

function fill(s, vars) {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (m, k) => (vars[k] !== undefined && vars[k] !== null ? String(vars[k]) : m));
}

/** Texto traducido. Las variables se escriben como {nombre} en el texto. */
export function t(key, vars) {
  return fill(lookup(i18n.lang, key) ?? key, vars);
}

const pluralRules = {};
/** Texto con plural según `n` (claves key.one / key.other / key.few…). `{n}` es el número formateado. */
export function tn(key, n, vars = {}) {
  const lang = i18n.lang;
  const rules = (pluralRules[lang] ??= new Intl.PluralRules(locale()));
  const form = rules.select(n);
  const s = DICTS[lang]?.[`${key}.${form}`] ?? DICTS[lang]?.[`${key}.other`] ?? lookup(lang, `${key}.${form === 'one' ? 'one' : 'other'}`) ?? key;
  return fill(s, { n: num(n), ...vars });
}

const escapeMap = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const esc = (v) => String(v).replace(/[&<>"']/g, (c) => escapeMap[c]);

/** Como t(), pero para textos con marcado simple (<strong>, <code>): escapa las variables. Úsalo con {@html}. */
export function th(key, vars) {
  const safe = vars ? Object.fromEntries(Object.entries(vars).map(([k, v]) => [k, esc(v)])) : undefined;
  return fill(lookup(i18n.lang, key) ?? key, safe);
}

/** Número con los separadores del idioma actual. */
export function num(n, opts) {
  return Number(n).toLocaleString(locale(), opts);
}
