// Última búsqueda: vive fuera de la vista para que, al ir a otra pestaña y volver, sigan ahí
// los resultados (y las búsquedas en curso terminen) sin tener que buscar de nuevo.
export const session = $state({
  query: '',
  lastQuery: '',
  searched: false,
  // Por indexador: { state: 'loading'|'ok'|'error', items, total, offset, count, ms, error }
  perIndexer: {},
  // Estado de acciones por enlace: 'working' | 'queued' | 'saving' | 'saved' | 'error'
  actions: {},
  expanded: null,
  scroll: 0,
});

// Contadores sin reactividad: descartan respuestas de búsquedas anteriores y agrupan las
// peticiones de una misma búsqueda en las estadísticas.
export const run = { generation: 0, searchId: null };
