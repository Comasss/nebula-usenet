//! Motor de Nebula sin interfaz: cola y descarga NNTP, post-procesado, categorías, debrid,
//! indexadores, RSS, APIs Newznab y SABnzbd, control remoto y paso de ajustes entre equipos.
//!
//! No depende de Tauri. Quien lo aloja (la app de escritorio y Android, el servidor) crea el
//! [`engine::Engine`] con su [`platform::Platform`], escucha el [`bus`] para mostrar los cambios
//! y llama a los métodos del motor (la fachada de `facade.rs`).
#[macro_use]
pub mod i18n;
pub mod logger;
pub mod api;
pub mod bus;
pub mod categories;
pub mod config;
pub mod debrid;
pub mod debrid_job;
pub mod engine;
pub mod facade;
pub mod fetch;
pub mod finish;
pub mod indexer;
pub mod nntp;
pub mod nzb;
pub mod platform;
pub mod postproc;
pub mod remote;
pub mod rss;
pub mod rt;
pub mod sab;
pub mod secrets;
pub mod stats;
pub mod sys;
pub mod tools;
pub mod transfer;
pub mod util;
pub mod yenc;

pub use bus::Event;
pub use engine::Engine;
pub use platform::{Headless, Platform};

/// Versión del motor (la misma que la de la app).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
