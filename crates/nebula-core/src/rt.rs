//! Runtime de tokio del motor. La app Tauri le pasa el suyo (`set_handle`) para que todo corra
//! donde antes; fuera de un runtime (pruebas, arranque) se usa uno propio creado al vuelo.
use std::future::Future;
use std::sync::{LazyLock, OnceLock};
use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle;

static HANDLE: OnceLock<Handle> = OnceLock::new();
static OWN: LazyLock<Runtime> = LazyLock::new(|| Runtime::new().expect("runtime de tokio"));

/// Usa este runtime para las tareas del motor que se lancen desde fuera de uno.
pub fn set_handle(h: Handle) {
    let _ = HANDLE.set(h);
}

pub fn handle() -> Handle {
    if let Ok(h) = Handle::try_current() {
        return h;
    }
    HANDLE.get().cloned().unwrap_or_else(|| OWN.handle().clone())
}

/// Lanza una tarea, se esté o no dentro de un runtime.
pub fn spawn<F>(f: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    handle().spawn(f)
}

/// Espera a un futuro desde código síncrono (no dentro de una tarea de tokio).
pub fn block_on<F: Future>(f: F) -> F::Output {
    match HANDLE.get() {
        Some(h) => h.block_on(f),
        None => OWN.block_on(f),
    }
}
