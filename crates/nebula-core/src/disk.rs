//! Escritura a disco de los artículos descargados.
//!
//! Antes cada conexión escribía su artículo con `spawn_blocking` y esperaba: con 20 o más
//! conexiones escribiendo en el mismo archivo, el núcleo las serializa con el cerrojo del inodo y
//! se iba mucha CPU girando en él (perfil de dev/bench.py). Ahora un único hilo escribe en orden
//! de llegada y marca cada segmento como hecho; la conexión no espera y sigue con el siguiente
//! artículo con otro búfer. La cola está limitada para que un disco lento frene la descarga en
//! vez de llenar la memoria.
use crate::engine::{write_all_at, Engine, Outcome, Task};
use parking_lot::Mutex;
use std::fs::File;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc;

/// Artículos esperando a escribirse (unos 25 MB con artículos de 750 KB).
const QUEUE: usize = 32;
/// Búferes que se guardan para reutilizar.
const POOL: usize = 48;

pub struct Write {
    pub handle: Arc<File>,
    pub data: Vec<u8>,
    pub offset: u64,
    pub task: Task,
    pub sidx: usize,
    pub gen: u64,
    pub crc_ok: bool,
}

#[derive(Default)]
pub struct DiskWriter {
    tx: Mutex<Option<mpsc::Sender<Write>>>,
    pool: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl DiskWriter {
    /// Un búfer vacío (reutilizado si hay alguno).
    pub fn buffer(&self) -> Vec<u8> {
        let mut b = self.pool.lock().pop().unwrap_or_default();
        b.clear();
        b
    }

    /// Encola la escritura. Cuando termina se llama a `Engine::complete` (o a `write_error`).
    /// Espera solo si la cola está llena.
    pub async fn submit(&self, e: &Arc<Engine>, w: Write) {
        let tx = {
            let mut g = self.tx.lock();
            match g.as_ref().filter(|t| !t.is_closed()) {
                Some(t) => t.clone(),
                None => {
                    let (tx, rx) = mpsc::channel(QUEUE);
                    let weak = Arc::downgrade(e);
                    let pool = self.pool.clone();
                    let spawned = std::thread::Builder::new().name("nebula-disk".into()).spawn(move || run(weak, rx, pool));
                    if spawned.is_err() {
                        // Sin hilo (no debería pasar): se escribe aquí mismo.
                        drop(g);
                        write_now(e, w);
                        return;
                    }
                    *g = Some(tx.clone());
                    tx
                }
            }
        };
        if let Err(mpsc::error::SendError(w)) = tx.send(w).await {
            write_now(e, w);
        }
    }
}

/// Escribe y marca el segmento; devuelve el búfer para reutilizarlo.
fn write_now(e: &Arc<Engine>, w: Write) -> Vec<u8> {
    match write_all_at(&w.handle, &w.data, w.offset) {
        Ok(()) => e.complete(&w.task, w.sidx, w.gen, Outcome::Ok { crc_ok: w.crc_ok }),
        Err(err) => e.write_error(&w.task, w.sidx, w.gen, err),
    }
    w.data
}

fn run(engine: Weak<Engine>, mut rx: mpsc::Receiver<Write>, pool: Arc<Mutex<Vec<Vec<u8>>>>) {
    while let Some(w) = rx.blocking_recv() {
        let Some(e) = engine.upgrade() else { break };
        let buf = write_now(&e, w);
        drop(e);
        let mut p = pool.lock();
        if p.len() < POOL && buf.capacity() > 0 {
            p.push(buf);
        }
    }
}
