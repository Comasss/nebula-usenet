//! `nebula`: Nebula desde la terminal.
//!
//! - Controla un nebula-server (en esta máquina o en otra) por su API: añadir NZB, ver la cola y
//!   el historial, pausar, borrar, limitar la velocidad, buscar en los indexadores…
//! - `nebula get` descarga un NZB sin servidor, con el motor dentro del propio programa.
//!
//! Códigos de salida (docs/CLI.md): 0 bien, 1 error, 2 uso incorrecto, 3 falló la descarga,
//! 4 falló la verificación o la reparación, 5 falló la extracción, 130 cancelado (Ctrl+C).
#[macro_use]
extern crate nebula_core;

mod client;
mod commands;
mod config;
mod fmt;
mod get;
#[cfg(feature = "tui")]
mod tui;

use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

pub const EXIT_ERROR: i32 = 1;
pub const EXIT_DOWNLOAD: i32 = 3;
pub const EXIT_REPAIR: i32 = 4;
pub const EXIT_EXTRACT: i32 = 5;
pub const EXIT_INTERRUPTED: i32 = 130;

/// Un error para el usuario y el código con el que sale el programa.
#[derive(Debug)]
pub struct Fail {
    pub code: i32,
    pub msg: String,
}

impl Fail {
    pub fn new(msg: impl Into<String>) -> Self {
        Fail { code: EXIT_ERROR, msg: msg.into() }
    }
    pub fn code(code: i32, msg: impl Into<String>) -> Self {
        Fail { code, msg: msg.into() }
    }
}

impl From<String> for Fail {
    fn from(msg: String) -> Self {
        Fail::new(msg)
    }
}

pub type Res<T = ()> = Result<T, Fail>;

#[derive(Parser, Debug)]
#[command(name = "nebula", version, about = "Nebula Usenet downloader from the terminal", propagate_version = true)]
pub struct Cli {
    /// Address of nebula-server (default: cli.toml, or http://localhost:5080).
    #[arg(long, env = "NEBULA_URL", global = true)]
    pub url: Option<String>,
    /// Nebula's API key (Settings → API). Default: cli.toml, or the local server's settings.
    #[arg(long, env = "NEBULA_API_KEY", global = true, hide_env_values = true)]
    pub api_key: Option<String>,
    /// Print JSON instead of text (for scripts).
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add .nzb files or links to the queue.
    Add {
        /// .nzb files (also .nzb.gz) or http(s) links.
        #[arg(required = true)]
        sources: Vec<String>,
        /// Category (tv, movies…). Without it, Default.
        #[arg(long, short)]
        cat: Option<String>,
        /// low, normal, high or force (default: the category's).
        #[arg(long, short)]
        priority: Option<String>,
        /// Download method: nntp, debrid, or the id of a server or debrid service.
        #[arg(long)]
        via: Option<String>,
        /// Name of the download (only with a single source).
        #[arg(long)]
        name: Option<String>,
        /// Add it paused.
        #[arg(long)]
        paused: bool,
        /// Add it even if it looks like a duplicate.
        #[arg(long)]
        force: bool,
    },
    /// Show the queue.
    Queue,
    /// Show the history.
    History {
        /// How many entries (newest first).
        #[arg(long, short = 'n', default_value_t = 20)]
        limit: usize,
        /// Only failed downloads.
        #[arg(long)]
        failed: bool,
    },
    /// Pause the whole queue, or one download.
    Pause {
        /// Id (or its start) or position in `nebula queue`.
        id: Option<String>,
    },
    /// Resume the whole queue, or one download.
    Resume {
        /// Id (or its start) or position in `nebula queue`.
        id: Option<String>,
    },
    /// Remove downloads from the queue or entries from the history.
    Rm {
        /// Ids (or their start) or positions in `nebula queue`.
        #[arg(required = true)]
        ids: Vec<String>,
        /// Also delete the downloaded files.
        #[arg(long)]
        delete_files: bool,
    },
    /// Show or set the speed limit: 10 (MB/s), 500K, 2.5M, 0 or off for no limit.
    Speed { limit: Option<String> },
    /// Search your indexers.
    Search {
        /// What to look for.
        #[arg(required = true)]
        query: Vec<String>,
        /// Only these indexers (id or name; repeatable).
        #[arg(long, short)]
        indexer: Vec<String>,
        /// Newznab categories (5000 = TV, 2000 = Movies…; comma separated).
        #[arg(long)]
        newznab: Option<String>,
        /// Add the result with this number to the queue.
        #[arg(long, short)]
        grab: Option<usize>,
        /// Category for the grabbed result (default: from its Newznab category).
        #[arg(long, short)]
        cat: Option<String>,
        /// How many results to show.
        #[arg(long, short = 'n', default_value_t = 25)]
        limit: usize,
    },
    /// Speed, queue and server status.
    Status,
    /// Download an NZB without a server and wait until it finishes.
    Get {
        /// The .nzb file.
        nzb: PathBuf,
        /// Folder for the download (a folder with its name is created inside).
        #[arg(long, short, default_value = ".")]
        output: PathBuf,
        /// Usenet server: nntps://user:password@host:563?conns=20 (repeatable; the first is the main one).
        /// Default: the servers of the local Nebula.
        #[arg(long, short)]
        server: Vec<String>,
        /// Archive password.
        #[arg(long)]
        password: Option<String>,
        /// Don't verify, repair or extract.
        #[arg(long)]
        no_pp: bool,
        /// Print the log as it happens.
        #[arg(long, short)]
        verbose: bool,
    },
    /// The queue in the terminal (arrows, p pause, space pause all, d remove, q quit).
    #[cfg(feature = "tui")]
    Tui,
    /// Shell completion script: bash, zsh, fish, powershell or elvish.
    Completions { shell: clap_complete::Shell },
}

#[tokio::main]
async fn main() {
    nebula_core::rt::set_handle(tokio::runtime::Handle::current());
    nebula_core::i18n::set(nebula_core::i18n::system_language());
    let cli = Cli::parse();
    let res = run(cli).await;
    if let Err(f) = res {
        eprintln!("{} {}", t!("cli.error"), f.msg);
        std::process::exit(f.code);
    }
}

async fn run(cli: Cli) -> Res {
    match &cli.command {
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), "nebula", &mut std::io::stdout());
            Ok(())
        }
        Command::Get { nzb, output, server, password, no_pp, verbose } => {
            get::run(nzb, output, server, password.clone(), *no_pp, *verbose, cli.json).await
        }
        #[cfg(feature = "tui")]
        Command::Tui => tui::run(config::connect(&cli)?).await,
        _ => commands::run(&cli, config::connect(&cli)?).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_add() {
        let c = Cli::try_parse_from(["nebula", "add", "a.nzb", "--cat", "tv", "--priority", "high", "--json"]).unwrap();
        assert!(c.json);
        match c.command {
            Command::Add { sources, cat, priority, .. } => {
                assert_eq!(sources, ["a.nzb"]);
                assert_eq!(cat.as_deref(), Some("tv"));
                assert_eq!(priority.as_deref(), Some("high"));
            }
            _ => panic!(),
        }
    }
}
