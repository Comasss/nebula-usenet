// Vista previa de la interfaz en un navegador normal, con la API de Tauri simulada.
// Uso: npm run dev  →  http://localhost:1420/dev/preview.html  (?setup=1 muestra el asistente, ?nodebrid sin servicios debrid, ?lang=en fija el idioma)
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';

const params = new URLSearchParams(location.search);
const now = () => Math.floor(Date.now() / 1000);
const GB = 1024 ** 3;
const MB = 1024 ** 2;

const settings = {
  featureNntp: !params.has('idxonly'),
  featureIndexers: true,
  featuresChosen: !params.has('feat'),
  indexers: [
    { id: 'i1', name: 'NZBGeek', url: 'https://api.nzbgeek.info', apiKey: 'x', apiKeyEnc: '', apiPath: '/api', enabled: true },
    { id: 'i2', name: 'NZBFinder', url: 'https://nzbfinder.ws', apiKey: 'x', apiKeyEnc: '', apiPath: '/api', enabled: true },
    { id: 'i3', name: 'DrunkenSlug', url: 'https://drunkenslug.com', apiKey: 'x', apiKeyEnc: '', apiPath: '/api', enabled: true },
  ],
  searchDedup: true,
  searchLimit: 100,
  searchQuickAdd: true,
  nzbSaveDir: 'D:\Descargas\NZB',
  nzbAskSaveDir: false,
  servers: [
    { id: 's1', name: 'Eweka', host: 'news.eweka.nl', port: 563, ssl: true, verifyCert: true, username: 'usuario', password: 'x', passwordEnc: '', connections: 40, priority: 0, enabled: true, retentionDays: 5800 },
    { id: 's2', name: 'Blocknews (bloque)', host: 'eu.blocknews.net', port: 563, ssl: true, verifyCert: true, username: 'usuario2', password: 'x', passwordEnc: '', connections: 10, priority: 1, enabled: true, retentionDays: 5500 },
  ],
  debrid: params.has('nodebrid')
    ? []
    : [
        { id: 'd1', kind: 'torbox', name: '', apiKey: 'tb-1234567890abcdef', apiKeyEnc: '', enabled: true },
        { id: 'd2', kind: 'premiumize', name: '', apiKey: 'pm-abcdef1234567890', apiKeyEnc: '', enabled: true },
      ],
  downloadVia: params.get('via') || 'nntp',
  debridFallbackNntp: true,
  debridConnections: 8,
  debridDeleteAfter: true,
  downloadDir: 'D:\\Descargas\\Nebula',
  incompleteDir: '',
  askDestination: true,
  createJobFolder: true,
  par2Enabled: true,
  extractEnabled: true,
  deleteArchives: true,
  deletePar2: true,
  abortHopeless: true,
  clipboardMonitor: true,
  clipboardPatterns: ['getnzb', '/api?t=get', '&t=get&', '/nzb/'],
  clipboardShowWindow: true,
  clipboardInspect: true,
  notifyComplete: true,
  notifyFailed: true,
  minimizeToTray: false,
  closeToTray: true,
  startMinimized: false,
  startWithWindows: false,
  associateNzb: true,
  speedLimitKbps: 0,
  maxRetries: 3,
  timeoutSecs: 30,
  animations: true,
  haptics: true,
  accent: params.get('accent') || 'nebula',
  language: params.get('lang') || '',
  confirmDelete: true,
  addPaused: false,
  historyLimit: 300,
  firstRunDone: !params.has('setup'),
  apiEnabled: params.has('api'),
  apiKey: '3f9c2a7e51d84b6a9c0e7f1d2b3a4c5d',
  apiPort: 5077,
  apiLan: true,
  rssFeeds: [{ id: 'f1', name: 'ShowRSS', url: 'https://showrss.example/feed.rss', indexer: '', intervalMin: 30, enabled: true }],
  rssRules: params.has('norss')
    ? []
    : [
        { id: 'r1', name: 'Some Show 1080p', enabled: true, feeds: [], must: 'some show 1080p', regex: false, exclude: 'hevc, x265', minSizeMb: 500, maxSizeMb: 6000, categories: '5000', via: '', destDir: 'D:\\Series\\Some Show', createdAt: now() - 5 * 86400, onlyNew: true },
        { id: 'r2', name: 'Documentales', enabled: false, feeds: ['ix-i1'], must: '^Planet\\..*2160p', regex: true, exclude: '', minSizeMb: 0, maxSizeMb: 0, categories: '', via: 'd1', destDir: '', createdAt: now() - 86400, onlyNew: false },
      ],
  finishActions: params.has('nofinish') ? [] : [
    { id: 'f1', kind: 'discord', name: '', enabled: true, onComplete: true, onFailed: true, onQueueEmpty: false, url: 'https://discord.com/api/webhooks/123/abc', token: '', chatId: '', topic: '', path: '', timeoutSecs: 120, power: 'shutdown', once: false },
    { id: 'f2', kind: 'telegram', name: 'Móvil', enabled: true, onComplete: false, onFailed: true, onQueueEmpty: true, url: '', token: '123456:ABC', chatId: '42', topic: '', path: '', timeoutSecs: 120, power: 'shutdown', once: false },
    { id: 'f3', kind: 'script', name: '', enabled: false, onComplete: true, onFailed: false, onQueueEmpty: false, url: '', token: '', chatId: '', topic: '', path: 'C:\\scripts\\mover-a-nas.bat', timeoutSecs: 300, power: 'shutdown', once: false },
    { id: 'f4', kind: 'power', name: '', enabled: params.has('armed'), onComplete: false, onFailed: false, onQueueEmpty: true, url: '', token: '', chatId: '', topic: '', path: '', timeoutSecs: 120, power: 'hibernate', once: true },
  ],
};

const rssItems = [
  ['Some.Show.S03E05.1080p.WEB.h264-GRP', 2.1 * GB, 'NZBGeek', 'TV > HD', 3600],
  ['Some.Show.S03E05.1080p.WEB.HEVC-OTHER', 1.2 * GB, 'NZBFinder', 'TV > HD', 4000],
  ['Some.Show.S03E05.720p.WEB.h264-GRP', 900 * MB, 'NZBGeek', 'TV > HD', 3700],
  ['Some.Show.S03E04.1080p.WEB.h264-GRP', 2 * GB, 'DrunkenSlug', 'TV > HD', 8 * 86400],
  ['Another.Movie.2026.2160p.UHD.BluRay.x265-ABC', 42 * GB, 'NZBGeek', 'Movies > UHD', 7200],
];
const rssLog = [
  { at: now() - 3 * 3600, ruleId: 'r1', rule: 'Some Show 1080p', title: 'Some.Show.S03E04.1080p.WEB.h264-GRP', indexer: 'NZBGeek', ok: true, error: null },
  { at: now() - 26 * 3600, ruleId: 'r1', rule: 'Some Show 1080p', title: 'Some.Show.S03E03.1080p.WEB.h264-GRP', indexer: 'NZBFinder', ok: false, error: 'No hay ningún indexador que dé el NZB' },
  { at: now() - 7 * 86400, ruleId: 'r1', rule: 'Some Show 1080p', title: 'Some.Show.S03E02.1080p.WEB.h264-GRP', indexer: 'DrunkenSlug', ok: true, error: null },
];
function rssFeedsView() {
  const ix = settings.featureIndexers ? settings.indexers.filter((i) => i.enabled).map((i) => {
    const saved = settings.rssFeeds.find((f) => f.id === `ix-${i.id}`);
    return { id: `ix-${i.id}`, name: i.name, url: '', indexer: i.id, intervalMin: saved?.intervalMin ?? 15, enabled: saved?.enabled ?? true };
  }) : [];
  const loose = settings.rssFeeds.filter((f) => !f.indexer);
  const rules = settings.rssRules.filter((r) => r.enabled);
  return [...ix, ...loose].map((f, n) => ({ ...f, used: rules.some((r) => !r.feeds.length || r.feeds.includes(f.id)), lastCheck: n === 2 ? 0 : now() - 240 * (n + 1), nextCheck: 0, lastError: n === 1 ? 'El indexador ha limitado las peticiones' : null, items: 100 }));
}
function rssPreview(rule) {
  const terms = (s) => s.split(/[\s,]+/).filter(Boolean).map((w) => w.toLowerCase());
  return rssItems.map(([title, size, indexer, category, age]) => {
    const words = ` ${title.toLowerCase().split(/[^a-z0-9]+/).join(' ')} `;
    const checks = [];
    if (rule.regex && rule.must) checks.push({ key: 'regex', ok: new RegExp(rule.must, 'i').test(title), detail: rule.must });
    else if (rule.must.trim()) { const miss = terms(rule.must).filter((w) => !words.includes(` ${w} `)); checks.push({ key: 'must', ok: !miss.length, detail: miss.length ? miss.join(', ') : terms(rule.must).join(', ') }); }
    if (rule.exclude.trim()) { const hit = terms(rule.exclude).filter((w) => words.includes(` ${w} `)); checks.push({ key: 'exclude', ok: !hit.length, detail: hit.join(', ') }); }
    if (rule.minSizeMb) checks.push({ key: 'minSize', ok: size / MB >= rule.minSizeMb, detail: String(rule.minSizeMb) });
    if (rule.maxSizeMb) checks.push({ key: 'maxSize', ok: size / MB <= rule.maxSizeMb, detail: String(rule.maxSizeMb) });
    if (rule.categories.trim()) checks.push({ key: 'category', ok: category.startsWith('TV'), detail: rule.categories });
    const date = now() - age;
    if (rule.onlyNew) checks.push({ key: 'date', ok: date >= rule.createdAt, detail: '' });
    const matched = checks.every((c) => c.ok);
    return { title, feed: indexer, indexer, size, date, category, matched, checks, seen: title.includes('S03E04'), duplicate: false };
  }).sort((a, b) => b.matched - a.matched);
}
if (params.has('setup')) settings.servers = [];

const jobs = [
  { id: 'j1', name: 'The.Expanse.S06E01.2160p.WEB-DL.DDP5.1.HDR.HEVC', status: 'downloading', total: 7.4 * GB, done: 3.1 * GB, failed: 12 * MB, failedArticles: 16, percent: 0, speed: 0, eta: 0, files: 78, filesDone: 31, heldPar2: 6, dest: 'D:\\Descargas\\Nebula', error: null, warning: null, post: null, addedAt: now() - 600 },
  { id: 'j2', name: 'Ubuntu.24.04.LTS.Desktop.amd64.ISO', status: 'extracting', total: 5.8 * GB, done: 5.8 * GB, failed: 0, failedArticles: 0, percent: 100, speed: 0, eta: 0, files: 62, filesDone: 62, heldPar2: 0, dest: 'D:\\Descargas\\Nebula', error: null, warning: null, post: { stage: 'extracting', percent: 38, speed: 312 * MB, detail: 'ubuntu-24.04.part01.rar' }, addedAt: now() - 1800 },
  { id: 'j3', name: 'Blender.Open.Movie.Collection.1080p', status: 'queued', total: 12.2 * GB, done: 0, failed: 0, failedArticles: 0, percent: 0, speed: 0, eta: 0, files: 131, filesDone: 0, heldPar2: 9, dest: 'E:\\Peliculas', error: null, warning: null, post: null, addedAt: now() - 120 },
  { id: 'j5', name: 'Planet.Earth.III.S01.2160p.UHD.BluRay.x265-DEBRID', status: 'downloading', total: 41.2 * GB, done: 30.1 * GB, failed: 0, failedArticles: 0, percent: 73, speed: 780 * MB, eta: 14, files: 0, filesDone: 0, heldPar2: 0, dest: 'E:\\Series', error: null, warning: null, post: null, addedAt: now() - 300, debrid: { provider: 'TorBox', kind: 'torbox', phase: 'remote', state: 'Descargando en TorBox', upload: 100, remote: 73 } },
  { id: 'j7', name: 'Some.Huge.Pack.2026.1080p.WEB-DL-DEBRID', status: 'downloading', total: 250 * GB, done: 0, failed: 0, failedArticles: 0, percent: 100, speed: 0, eta: 0, files: 0, filesDone: 0, heldPar2: 0, dest: 'E:\\Series', error: null, warning: null, post: null, addedAt: now() - 60, debrid: { provider: 'TorBox', kind: 'torbox', phase: 'submit', state: 'TorBox está analizando el NZB · 0:42', upload: 100, remote: 0, waiting: true } },
  { id: 'j6', name: 'Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR10.DDP5.1.Atmos', status: 'downloading', total: 22.8 * GB, done: 9.4 * GB, failed: 0, failedArticles: 0, percent: 41, speed: 0, eta: 0, files: 3, filesDone: 1, heldPar2: 0, dest: 'E:\\Peliculas', error: null, warning: null, post: null, addedAt: now() - 900, debrid: { provider: 'Premiumize', kind: 'premiumize', phase: 'fetch', state: 'Bajando desde Premiumize', upload: 100, remote: 100 } },
  { id: 'j4', name: 'Podcast.Archivo.2025.FLAC', status: 'paused', total: 2.1 * GB, done: 0.4 * GB, failed: 0, failedArticles: 0, percent: 19, speed: 0, eta: 0, files: 24, filesDone: 4, heldPar2: 3, dest: 'D:\\Descargas\\Nebula', error: 'Error de escritura en disco: acceso denegado (os error 5)', warning: null, post: null, addedAt: now() - 90 },
];

function snapshot() {
  const j1 = jobs[0];
  const speed = paused ? 0 : 88 * MB + Math.sin(Date.now() / 1300) * 22 * MB + Math.random() * 8 * MB;
  if (!paused && j1.status === 'downloading') {
    j1.done = Math.min(j1.total, j1.done + speed * 0.5);
    j1.speed = speed;
    j1.percent = ((j1.done + j1.failed) / j1.total) * 100;
    j1.eta = (j1.total - j1.done) / speed;
  } else {
    j1.speed = 0;
  }
  const j6 = jobs.find((j) => j.id === 'j6');
  if (!paused && j6) {
    const sp = 240 * MB + Math.random() * 40 * MB;
    j6.speed = sp;
    j6.done = Math.min(j6.total, j6.done + sp * 0.5);
    j6.percent = (j6.done / j6.total) * 100;
    j6.eta = (j6.total - j6.done) / sp;
  }
  const j2 = jobs[1];
  if (j2.post) {
    j2.post.percent = (j2.post.percent + 0.6) % 100;
    j2.post.speed = 280 * MB + Math.random() * 60 * MB;
  }
  const remaining = jobs.filter((j) => ['queued', 'downloading'].includes(j.status)).reduce((a, j) => a + j.total - j.done, 0);
  return {
    paused,
    speed: paused ? 0 : speed,
    remaining,
    eta: speed ? remaining / speed : 0,
    speedLimit: settings.speedLimitKbps,
    activeConns: paused ? 0 : 38,
    noServers: settings.servers.length === 0,
    jobs: structuredClone(jobs),
    servers: settings.servers.map((s, i) => ({
      id: s.id, name: s.name, priority: s.priority, connected: i ? 2 : 40, active: paused ? 0 : i ? 1 : 37, maxConns: s.connections,
      state: paused ? 'idle' : 'active', lastError: null, articlesOk: i ? 16 : 48213, articlesMissing: i ? 0 : 16, bytes: i ? 11 * MB : 34.6 * GB,
    })),
  };
}
let paused = false;

const history = [
  { id: 'h1', name: 'Arch.Linux.2026.09.01.x86_64', status: 'completed', size: 1.2 * GB, dest: 'D:\\Descargas\\Nebula\\Arch.Linux.2026.09.01.x86_64', destDir: 'D:\\Descargas\\Nebula', workDir: '', nzbPath: '', error: null, warning: null, failedStage: null, canRetryExtract: false, password: null, options: {}, addedAt: now() - 7200, completedAt: now() - 6900, downloadSecs: 21, avgSpeed: 58 * MB },
  { id: 'h5', name: 'Oppenheimer.2023.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos', status: 'completed', size: 71.3 * GB, dest: 'E:\\Peliculas\\Oppenheimer', destDir: 'E:\\Peliculas', workDir: '', nzbPath: '', error: null, warning: null, failedStage: null, canRetryExtract: false, password: null, options: {}, addedAt: now() - 5400, completedAt: now() - 4700, downloadSecs: 610, avgSpeed: 120 * MB, via: 'd1', viaName: 'TorBox' },
  { id: 'h2', name: 'Documental.Cosmos.S01.1080p.BluRay', status: 'completed', size: 38.4 * GB, dest: 'E:\\Series\\Cosmos', destDir: 'E:\\Series', workDir: '', nzbPath: '', error: null, warning: 'Faltan 3 artículos y se repararon con PAR2.', failedStage: null, canRetryExtract: false, password: null, options: {}, addedAt: now() - 90000, completedAt: now() - 86000, downloadSecs: 402, avgSpeed: 97 * MB },
  { id: 'h3', name: 'Coleccion.Privada.Protegida', status: 'failed', size: 4.4 * GB, dest: '', destDir: 'D:\\Descargas\\Nebula', workDir: 'D:\\Descargas\\Nebula\\_incompleto\\Coleccion.Privada.Protegida', nzbPath: '', error: 'El archivo está protegido con contraseña. Introdúcela y reintenta la extracción.', warning: null, failedStage: 'extract', canRetryExtract: true, password: null, options: {}, addedAt: now() - 20000, completedAt: now() - 19000, downloadSecs: 80, avgSpeed: 55 * MB },
  { id: 'h4', name: 'Vieja.Publicacion.2009.DVDRip', status: 'failed', size: 0.7 * GB, dest: '', destDir: '', workDir: '', nzbPath: '', error: 'Faltan demasiados artículos en el servidor y los archivos PAR2 no bastan para repararlo. Se ha cancelado para no gastar ancho de banda.', warning: null, failedStage: 'download', canRetryExtract: false, password: null, options: {}, addedAt: now() - 300000, completedAt: now() - 299000, downloadSecs: 30, avgSpeed: 20 * MB },
];

const logs = [
  ['info', 'Nebula 1.0.0 iniciado'],
  ['info', 'Añadido a la cola: The.Expanse.S06E01.2160p.WEB-DL.DDP5.1.HDR.HEVC'],
  ['warn', 'Blocknews (bloque): Error de conexión: el servidor cerró la conexión'],
  ['info', 'Conexión restablecida con Blocknews (bloque)'],
  ['info', '«Ubuntu.24.04.LTS.Desktop.amd64.ISO»: verificación correcta'],
  ['error', 'Falló «Coleccion.Privada.Protegida»: El archivo está protegido con contraseña.'],
].map(([level, msg], i) => ({ ts: Date.now() - (60 - i) * 1000, level, msg }));

const files = Array.from({ length: 14 }, (_, i) => ({
  name: i === 0 ? 'The.Expanse.S06E01.par2' : i > 11 ? `The.Expanse.S06E01.vol${String((i - 12) * 32).padStart(3, '0')}+32.par2` : `The.Expanse.S06E01.part${String(i).padStart(2, '0')}.rar`,
  bytes: i === 0 ? 40 * 1024 : 100 * MB,
  done: i < 5 ? 100 * MB : i === 5 ? 43 * MB : 0,
  failed: i === 3 ? 1.5 * MB : 0,
  segments: 140,
  failedSegs: i === 3 ? 2 : 0,
  held: i > 11,
  par2: i === 0 || i > 11,
  complete: i < 5,
}));

// Estadísticas de ejemplo: un año de descargas con dos servidores y TorBox.
const stats = (() => {
  let seed = 7;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  const out = [];
  for (let i = 0; i < 420; i++) {
    const at = now() - Math.floor(rnd() * 400 * 86400);
    const debrid = rnd() < 0.15;
    const old = at < now() - 200 * 86400;
    const ok = rnd() < (debrid ? 0.97 : 0.86);
    const parts = 3000 + Math.floor(rnd() * 9000);
    const miss1 = ok ? Math.floor(rnd() * 40) : Math.floor(parts * (0.05 + rnd() * 0.3));
    const servers = debrid || old ? [] : [
      { id: 's1', name: 'Eweka', ok: parts - miss1, missing: miss1, bytes: (parts - miss1) * 740 * 1024 },
      ...(miss1 ? [{ id: 's2', name: 'Blocknews (bloque)', ok: Math.floor(miss1 * (ok ? 0.9 : 0.3)), missing: Math.ceil(miss1 * (ok ? 0.1 : 0.7)), bytes: miss1 * 700 * 1024 }] : []),
      ...(rnd() < 0.1 ? [{ id: 's9', name: 'Newshosting', ok: 50, missing: 10, bytes: 50 * 700 * 1024 }] : []),
    ];
    const indexer = rnd() < 0.7 ? ['i1', 'i2', 'i3'][Math.floor(rnd() * 3)] : null;
    out.push({ id: 'r' + i, name: 'nzb ' + i, at, ok, size: parts * 740 * 1024, via: debrid ? 'd1' : 'nntp', viaName: debrid ? 'TorBox' : null, failedStage: ok ? null : 'download', servers, indexer });
  }
  return out.sort((a, b) => a.at - b.at);
})();

// Búsquedas de ejemplo en los tres indexadores.
const indexerStats = (() => {
  let seed = 11;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  const IX = [['i1', 'NZBGeek', 650, 0.01, 60], ['i2', 'NZBFinder', 1250, 0.04, 35], ['i3', 'DrunkenSlug', 2100, 0.12, 48], ['i9', 'NZBPlanet', 900, 0.02, 20]];
  const out = [];
  for (let s = 0; s < 900; s++) {
    const at = now() - Math.floor(rnd() * 380 * 86400);
    for (const [id, name, ms, err, res] of IX) {
      if (id === 'i9' && at > now() - 250 * 86400) continue;
      const ok = rnd() > err;
      out.push({ at, id, name, kind: 'search', ms: Math.round(ms * (0.5 + rnd())), ok, results: ok ? Math.floor(rnd() * res * 2) : 0, offset: 0, search: 's' + s });
    }
    if (rnd() < 0.4) {
      const [id, name] = IX[Math.floor(rnd() * 3)];
      out.push({ at: at + 30, id, name, kind: 'grab', ms: 400, ok: rnd() > 0.05, results: 0, offset: 0, search: null });
    }
  }
  return out.sort((a, b) => a.at - b.at);
})();

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const remotePcs = params.has('pcs') ? [{ id: 'pc1', name: 'SOBREMESA', addr: '192.168.1.20:5078', pairedAt: now() - 86400 * 9 }] : [];

mockWindows('main');
mockIPC(
  async (cmd, args) => {
    switch (cmd) {
      case 'get_settings': return structuredClone(settings);
      case 'rss_status': return { feeds: rssFeedsView(), log: rssLog, busy: false, paused };
      case 'rss_check_now': await sleep(900); return { feeds: rssFeedsView().filter((f) => f.used && f.enabled).length, grabbed: 1, errors: 0 };
      case 'rss_preview': await sleep(500); return { items: rssPreview(args.rule), errors: [] };
      case 'rss_clear_log': rssLog.length = 0; return null;
      case 'save_settings': Object.assign(settings, args.settings); return null;
      case 'get_queue': return snapshot();
      case 'get_history': return structuredClone(history);
      case 'get_stats': return params.has('nostats') ? [] : stats;
      case 'get_indexer_stats': return params.has('nostats') ? [] : indexerStats;
      case 'get_logs': return logs;
      case 'app_info': return { version: '1.0.0', dataDir: 'C:\\Users\\demo\\AppData\\Roaming\\Nebula', toolsDir: '', logDir: '' };
      case 'get_job_files': return files;
      case 'set_paused': paused = args.paused; return null;
      case 'set_speed_limit': settings.speedLimitKbps = args.kbps; return null;
      case 'disk_free': return 812 * GB;
      case 'default_patterns': return ['getnzb', '/api?t=get', '&t=get&', '/nzb/'];
      case 'test_server':
        await sleep(900);
        return args.server.host.includes('mal')
          ? { ok: false, message: 'Autenticación rechazada (481 Authentication failed). Revisa usuario y contraseña.', latencyMs: 120 }
          : { ok: true, message: 'Conexión correcta: 200 Welcome to the news server', latencyMs: 34 };
      case 'inspect_nzb_url':
      case 'inspect_nzb_file':
        await sleep(900);
        if ((args.url ?? '').includes('login')) throw 'El enlace devuelve una página web, no un NZB. Puede que requiera iniciar sesión o que no sea un enlace directo.';
        return {
          token: 't1', name: (args.url ?? '').includes('nzbgeek') ? 'Breaking.Bad.2008.S01E01.Pilot.2160p.NF.WEB-DL.DUAL.DDP5.1.H.265-FUZEER' : 'Nasa.Earth.From.Space.2160p.HDR', source: args.url ?? args.path, password: null,
          totalBytes: 9.1 * GB, dataBytes: 8.6 * GB, par2Bytes: 0.5 * GB, par2Blocks: 380, fileCount: 96, segmentCount: 12000,
          hasArchives: true, groups: ['alt.binaries.hdtv'], oldestPost: now() - 86400 * 3,
          files: Array.from({ length: 12 }, (_, i) => ({ name: `nasa.earth.part${String(i + 1).padStart(2, '0')}.rar`, bytes: 100 * MB, par2: false })),
          defaultDir: settings.downloadDir, retentionWarning: null,
        };
      case 'add_job': return 'new';
      case 'debrid_cached': await sleep(600); return ['d1'];
      case 'test_debrid':
        await sleep(800);
        return args.provider.kind === 'torbox'
          ? { ok: true, message: 'Conectado como demo@correo.com · plan Pro hasta 12/03/2027', warning: null, latencyMs: 180 }
          : { ok: true, message: 'Conectado · premium hasta 01/02/2027 · 12 % del límite usado', warning: null, latencyMs: 240 };
      case 'search_indexer': {
        await sleep({ i1: 700, i2: 1300, i3: 1900 }[args.id] ?? 800);
        if (args.id === 'i3') throw 'Límite de peticiones a la API alcanzado; inténtalo más tarde';
        const q = args.query || 'Breaking Bad';
        const base = [
          ['Breaking.Bad.S01E01.Pilot.2160p.NF.WEB-DL.DDP5.1.H.265-FUZEER', 8.6, 3, 1204, [5000, 5045]],
          ['Breaking.Bad.S01E02.Cats.in.the.Bag.2160p.NF.WEB-DL.DDP5.1.H.265-FUZEER', 7.9, 3, 980, [5000, 5045]],
          ['Breaking.Bad.S01.1080p.BluRay.x264-ROVERS', 26.4, 400, 3120, [5000, 5040]],
          ['Breaking.Bad.The.Movie.El.Camino.2019.2160p.WEB-DL.DDP5.1.HDR.HEVC-GRP', 19.2, 40, 2205, [2000, 2045]],
          ['Breaking.Bad.Complete.Series.720p.WEB-DL-MIXED', 68.1, 900, 77, [5000, 5040]],
        ];
        const items = base
          .filter((_, k) => args.id === 'i1' || k % 2 === 0)
          .map(([title, gb, days, grabs, cats], k) => ({
            title: args.id === 'i2' && k === 0 ? title + '-Obfuscated' : title,
            link: `https://${args.id}.example/getnzb/${k}?apikey=x`,
            details: `https://${args.id}.example/details/${k}`,
            size: Math.round(gb * GB * (args.id === 'i2' ? 1.004 : 1)),
            date: now() - days * 86400,
            grabs: args.id === 'i2' ? Math.round(grabs / 3) : grabs,
            files: 80,
            category: 'TV > UHD',
            categoryIds: cats,
            password: k === 3 && args.id === 'i1' ? 1 : 0,
            indexerId: args.id,
            indexerName: settings.indexers.find((i) => i.id === args.id).name,
          }));
        return { items: q ? items : items, total: items.length, offset: 0, ms: { i1: 640, i2: 1210 }[args.id] };
      }
      case 'test_indexer':
        await sleep(800);
        return { ok: true, message: `Conectado a ${args.indexer.name || 'indexador'} · hasta 100 resultados por búsqueda`, latencyMs: 212 };
      case 'save_nzb_url':
      case 'save_pending_nzb':
        await sleep(500);
        return args.dir + '\\' + args.name + '.nzb';
      case 'plugin:window|is_maximized': return false;
      // Acciones al terminar (con ?armed el apagado rápido está activo; con #power, la ventana de la cuenta atrás)
      case 'finish_test':
        await sleep(700);
        if (args.action.kind === 'telegram') throw 'HTTP 401: {"ok":false,"error_code":401,"description":"Unauthorized"}';
        return null;
      case 'power_status': return location.hash === '#power' ? { mode: 'shutdown', remaining: 42, test: false } : null;
      case 'power_cancel': return true;
      case 'power_quick': {
        const p = settings.finishActions.find((a) => a.kind === 'power');
        if (args.mode) Object.assign(p, { enabled: true, power: args.mode, onQueueEmpty: true });
        else p.enabled = false;
        return structuredClone(settings);
      }

      // Android (con ?android y el user agent de un móvil)
      case 'app_resumed': return { files: [], urls: [], clipboard: [], storage: !params.has('nostorage') };
      case 'request_storage_access': return true;
      case 'list_dirs': {
        const p = args.path.replace(/\/+$/, '');
        const tree = { '/storage/emulated/0': ['Alarms', 'DCIM', 'Documents', 'Download', 'Movies', 'Music', 'Pictures'], '/storage/emulated/0/Download': ['Nebula', 'NZB', 'Telegram'], '/storage/emulated/0/Movies': ['Series'] };
        return { path: p, parent: p.replace(/\/[^/]+$/, '') || '/', dirs: tree[p] ?? [] };
      }
      case 'make_dir': return null;
      case 'api_status': return settings.apiEnabled ? { running: true, error: null, port: 5077, urls: ['http://localhost:5077/api', 'http://192.168.1.20:5077/api'] } : { running: false, error: null, port: 5077, urls: [] };
      // Actualizaciones (con ?update hay una versión nueva)
      case 'update_check':
        await sleep(500);
        return {
          available: params.has('update'),
          current: '1.0.0',
          version: '1.6.0',
          notes: '<!-- nebula-autobuild -->\n## Cambios\n- Actualizador integrado con aviso y descarga desde la app\n- API compatible con NZBHydra2\n- Corrige la reanudación de descargas en Android\n\n| File | What |\n|---|---|',
          url: 'https://github.com/Comasss/nebula/releases/tag/v1.6.0',
          publishedAt: new Date().toISOString(),
          kind: /android/i.test(navigator.userAgent) ? 'apk' : 'installer',
          assetName: 'Nebula_1.6.0_x64-setup.exe',
          assetSize: 14.2 * MB,
        };
      case 'update_download': {
        const total = 14.2 * MB;
        for (let i = 1; i <= 40; i++) {
          await sleep(120);
          emit('update-progress', { downloaded: Math.round((total * i) / 40), total });
        }
        return null;
      }
      case 'update_install': await sleep(300); return { started: true, needsPermission: false };
      // Control remoto (en Android, con ?android; ?pcs = ya hay un PC emparejado)
      case 'remote_status': return settings.remoteEnabled ? { running: true, error: null, warning: null, port: 5078, name: 'SOBREMESA', addresses: ['192.168.1.20:5078'] } : { running: false, port: 5078, name: 'SOBREMESA', addresses: [] };
      case 'remote_devices': return settings.remoteEnabled ? [{ id: 'm1', name: 'Pixel 8', pairedAt: now() - 86400 * 9, lastSeen: now() - 60, online: true }, { id: 'm2', name: 'SM-S918B', pairedAt: now() - 86400 * 40, lastSeen: now() - 86400 * 3, online: false }] : [];
      case 'remote_pcs': return remotePcs;
      case 'remote_discover':
        await sleep(1500);
        return [
          { id: 'pc1', name: 'SOBREMESA', version: '1.7.0', addr: '192.168.1.20:5078', paired: remotePcs.some((p) => p.id === 'pc1') },
          { id: 'pc2', name: 'PORTATIL-GENIS', version: '1.7.0', addr: '192.168.1.34:5078', paired: false },
        ];
      case 'remote_pair': {
        await sleep(600);
        emit('remote-pair-code', { code: '482913', name: args.addr.includes('34') ? 'PORTATIL-GENIS' : 'SOBREMESA' });
        await sleep(params.has('pairwait') ? 600000 : 2500);
        const pc = { id: args.addr.includes('34') ? 'pc2' : 'pc1', name: args.addr.includes('34') ? 'PORTATIL-GENIS' : 'SOBREMESA', addr: args.addr, pairedAt: now() };
        remotePcs.push(pc);
        return pc;
      }
      case 'remote_forget': remotePcs.splice(remotePcs.findIndex((p) => p.id === args.id), 1); return null;
      case 'remote_connect':
        setTimeout(() => {
          emit('remote-state', { id: args.id, name: 'SOBREMESA', status: params.has('offline') ? 'offline' : 'online', error: params.has('offline') ? 'No hay conexión con SOBREMESA: puede que esté apagado, dormido o con Nebula cerrado.' : null });
          if (!params.has('offline')) { emit('remote-queue', snapshot()); emit('remote-history', history); }
        }, 700);
        return { id: args.id, name: 'SOBREMESA', status: 'connecting', error: null };
      case 'remote_call': {
        const m = { get_vias: { servers: [{ id: 'ps1', name: 'Eweka', host: 'news.eweka.nl', priority: 0 }], debrid: [{ id: 'pd1', kind: 'torbox', name: '' }] }, get_queue: snapshot(), get_history: history, get_stats: stats, get_indexer_stats: indexerStats, get_job_files: files };
        return m[args.op] ?? null;
      }
      case 'remote_send': await sleep(700); return { id: 'x', name: 'Nasa.Earth.From.Space.2160p.HDR' };
      default: return null;
    }
  },
  { shouldMockEvents: true }
);

await import('../src/main.js');

setInterval(() => emit('queue', snapshot()), 500);
if (params.has('clipboard')) setTimeout(() => emit('clipboard-nzb', ['https://api.nzbgeek.info/api?t=get&id=8f2a91c&apikey=demo']), 1500);
if (params.has('clipbad')) setTimeout(() => emit('clipboard-nzb', ['https://indexador.example/login?next=/nzb/1']), 1500);
if (params.has('add')) setTimeout(() => emit('open-items', [{ kind: 'url', value: 'https://indexador.example/nzb/nasa-earth.nzb' }]), 1200);
if (params.has('toast')) setTimeout(() => emit('toast', { kind: 'success', message: 'Completado: Arch.Linux.2026.09.01.x86_64' }), 1000);
window.__nebulaPreview = { emit, snapshot, jobs };
