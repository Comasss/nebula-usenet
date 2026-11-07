// Estadísticas de descargas: un registro por NZB terminado (correcto o fallido), con el método usado
// y los artículos que sirvió cada servidor NNTP. Se guardan aparte del historial para que no se
// pierdan al limpiarlo ni al llegar a su límite.
use crate::config::{write_atomic, AppPaths};
use crate::engine::HistoryEntry;
use crate::lerror;
use serde::{Deserialize, Serialize};

/// Registros que se conservan como máximo (los más antiguos se descartan).
const MAX_RECORDS: usize = 200_000;

/// Artículos que un servidor NNTP sirvió (o no tenía) para un NZB.
#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerUse {
    pub id: String,
    pub name: String,
    pub ok: u64,
    pub missing: u64,
    pub bytes: u64,
}

impl ServerUse {
    fn add(&mut self, o: &ServerUse) {
        self.name = o.name.clone();
        self.ok += o.ok;
        self.missing += o.missing;
        self.bytes += o.bytes;
    }
}

/// Suma los artículos de un servidor en la lista de un trabajo.
pub fn count(list: &mut Vec<ServerUse>, id: &str, name: &str, ok: bool, bytes: u64) {
    let pos = match list.iter().position(|u| u.id == id) {
        Some(p) => p,
        None => {
            list.push(ServerUse { id: id.to_string(), name: name.to_string(), ..Default::default() });
            list.len() - 1
        }
    };
    let u = &mut list[pos];
    if ok {
        u.ok += 1;
        u.bytes += bytes;
    } else {
        u.missing += 1;
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct StatRecord {
    /// Id del primer intento: los reintentos actualizan el mismo registro.
    pub id: String,
    pub name: String,
    /// Fecha en que terminó (segundos Unix).
    pub at: u64,
    pub ok: bool,
    pub size: u64,
    /// "nntp" o el id del servicio debrid.
    pub via: String,
    pub via_name: Option<String>,
    pub failed_stage: Option<String>,
    pub servers: Vec<ServerUse>,
    /// Indexador del que se obtuvo el NZB, si vino de una búsqueda.
    pub indexer: Option<String>,
}

impl StatRecord {
    pub fn from_history(h: &HistoryEntry, servers: Vec<ServerUse>) -> Self {
        Self {
            id: h.stats_id.clone().unwrap_or_else(|| h.id.clone()),
            name: h.name.clone(),
            at: h.completed_at,
            ok: h.status == "completed",
            size: h.size,
            via: h.via.clone().unwrap_or_else(|| "nntp".into()),
            via_name: h.via_name.clone(),
            failed_stage: h.failed_stage.clone(),
            servers,
            indexer: h.indexer.clone(),
        }
    }
}

/// Petición a un indexador: una búsqueda (o página de resultados) o la descarga de un NZB.
#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct IndexerEvent {
    pub at: u64,
    pub id: String,
    pub name: String,
    /// "search" o "grab".
    pub kind: String,
    pub ms: u64,
    pub ok: bool,
    /// Resultados recibidos (búsquedas).
    pub results: u64,
    /// Posición de la página (0 = primera página de la búsqueda).
    pub offset: u32,
    /// Agrupa las peticiones a varios indexadores de una misma búsqueda.
    pub search: Option<String>,
}

const MAX_EVENTS: usize = 100_000;

fn indexer_file(paths: &AppPaths) -> std::path::PathBuf {
    paths.data.join("indexer_stats.json")
}

pub fn load_indexer(paths: &AppPaths) -> Vec<IndexerEvent> {
    std::fs::read(indexer_file(paths)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

pub fn save_indexer(paths: &AppPaths, events: &[IndexerEvent]) {
    match serde_json::to_vec(events) {
        Ok(d) => {
            if let Err(e) = write_atomic(&indexer_file(paths), &d) {
                lerror!("{}", e);
            }
        }
        Err(e) => lerror!("{}", e),
    }
}

pub fn push_indexer(events: &mut Vec<IndexerEvent>, ev: IndexerEvent) {
    events.push(ev);
    if events.len() > MAX_EVENTS {
        let extra = events.len() - MAX_EVENTS;
        events.drain(..extra);
    }
}

fn file(paths: &AppPaths) -> std::path::PathBuf {
    paths.data.join("stats.json")
}

/// Carga las estadísticas. La primera vez se crean a partir del historial existente
/// (sin el detalle por servidor, que no se guardaba).
pub fn load(paths: &AppPaths, history: &[HistoryEntry]) -> Vec<StatRecord> {
    match std::fs::read(file(paths)) {
        Ok(data) => serde_json::from_slice(&data).unwrap_or_default(),
        Err(_) => {
            let mut v: Vec<StatRecord> = history.iter().map(|h| StatRecord::from_history(h, Vec::new())).collect();
            v.sort_by_key(|r| r.at);
            save(paths, &v);
            v
        }
    }
}

pub fn save(paths: &AppPaths, records: &[StatRecord]) {
    match serde_json::to_vec(records) {
        Ok(d) => {
            if let Err(e) = write_atomic(&file(paths), &d) {
                lerror!("{}", e);
            }
        }
        Err(e) => lerror!("{}", e),
    }
}

/// Añade un registro, o actualiza el de un intento anterior del mismo NZB sumando sus servidores.
pub fn upsert(records: &mut Vec<StatRecord>, mut rec: StatRecord) {
    if let Some(pos) = records.iter().rposition(|r| r.id == rec.id) {
        let old = records.remove(pos);
        let mut servers = old.servers;
        for s in &rec.servers {
            match servers.iter_mut().find(|u| u.id == s.id) {
                Some(u) => u.add(s),
                None => servers.push(s.clone()),
            }
        }
        rec.servers = servers;
    }
    records.push(rec);
    if records.len() > MAX_RECORDS {
        let extra = records.len() - MAX_RECORDS;
        records.drain(..extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, ok: bool, servers: Vec<ServerUse>) -> StatRecord {
        StatRecord { id: id.into(), ok, via: "nntp".into(), servers, ..Default::default() }
    }

    #[test]
    fn count_por_servidor() {
        let mut v = Vec::new();
        count(&mut v, "a", "A", true, 10);
        count(&mut v, "a", "A", false, 10);
        count(&mut v, "b", "B", true, 5);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].ok, v[0].missing, v[0].bytes), (1, 1, 10));
        assert_eq!((v[1].ok, v[1].missing, v[1].bytes), (1, 0, 5));
    }

    #[test]
    fn reintento_actualiza_el_mismo_registro() {
        let mut list = Vec::new();
        let s = |ok, missing| ServerUse { id: "a".into(), name: "A".into(), ok, missing, bytes: 0 };
        upsert(&mut list, rec("x", false, vec![s(5, 3)]));
        upsert(&mut list, rec("y", true, vec![]));
        upsert(&mut list, rec("x", true, vec![s(2, 0)]));
        assert_eq!(list.len(), 2);
        let x = list.iter().find(|r| r.id == "x").unwrap();
        assert!(x.ok);
        assert_eq!((x.servers[0].ok, x.servers[0].missing), (7, 3));
    }
}
