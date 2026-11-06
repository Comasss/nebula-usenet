//! Los comandos de la interfaz por HTTP: `POST /api/v1/invoke/<comando>` con los mismos nombres y
//! argumentos (en camelCase) que los comandos de la app Tauri, para que la interfaz sea la misma.
//! Lo que solo tiene sentido con ventana (minimizar, abrir carpetas en el Explorador, actualizar
//! el ejecutable…) no hace nada o responde con un error que la interfaz no llega a mostrar porque
//! oculta esas opciones en el navegador.
use nebula_core::config::{DebridConfig, FinishAction, IndexerConfig, RssRule, ServerConfig, Settings};
use nebula_core::engine::{AddRequest, Engine};
use nebula_core::{api, debrid, engine, indexer, logger, remote, rss, transfer};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Argumento `key` de la llamada (si falta, como `null`: vale para los opcionales).
fn arg<T: DeserializeOwned>(a: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(a.get(key).cloned().unwrap_or(Value::Null)).map_err(|e| format!("{}: {}", key, e))
}

fn ok<T: Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn unavailable() -> Result<Value, String> {
    Err(t!("server.notAvailable"))
}

/// Ejecuta un comando. `Err` lleva el texto que la interfaz muestra.
pub async fn call(e: &Arc<Engine>, cmd: &str, a: &Value) -> Result<Value, String> {
    match cmd {
        // ---- Ajustes y aplicación ----
        "get_settings" => ok(e.settings.read().clone()),
        "save_settings" => ok(e.save_settings(arg::<Settings>(a, "settings")?)?),
        "set_language" => ok(e.set_language(&arg::<String>(a, "lang")?)),
        "app_info" => ok(e.app_info()),
        "app_ready" => {
            e.ui_ready();
            ok(())
        }
        "app_resumed" => ok(json!({ "files": [], "urls": [], "clipboard": [], "storage": true })),
        "request_storage_access" => ok(true),
        "window_minimize" | "window_close" | "quit_app" | "app_background" | "haptic" | "update_cancel" => ok(()),
        "open_path" | "reveal_path" | "open_url" | "update_check" | "update_download" | "update_install" => unavailable(),
        "get_logs" => ok(logger::entries()),
        "clear_logs" => ok(logger::clear()),
        "disk_free" => ok(nebula_core::sys::disk_free(std::path::Path::new(&arg::<String>(a, "path")?))),
        "default_patterns" => ok(nebula_core::config::default_patterns()),
        "detect_nzb_urls" => ok(e.detect_nzb_urls(&arg::<String>(a, "text")?)),
        "list_dirs" => ok(nebula_core::facade::list_dirs(&arg::<String>(a, "path")?)?),
        "make_dir" => ok(nebula_core::facade::make_dir(&arg::<String>(a, "path")?)?),
        "api_status" => ok(api::status()),
        "tools_status" => ok(e.tools_status()),

        // ---- Añadir ----
        "test_server" => ok(engine::test_server(arg::<ServerConfig>(a, "server")?).await),
        "inspect_nzb_file" => ok(e.inspect_nzb_file(arg(a, "path")?).await?),
        "inspect_nzb_url" => ok(e.inspect_nzb_url(arg(a, "url")?, arg(a, "indexerId")?).await?),
        "discard_pending" => ok(e.discard_pending(&arg::<String>(a, "token")?)),
        "add_job" => ok(e.add_job(arg::<AddRequest>(a, "req")?)?),
        "save_nzb_url" => ok(e
            .save_nzb_url(&arg::<String>(a, "url")?, &arg::<String>(a, "dir")?, &arg::<String>(a, "name")?, arg::<Option<String>>(a, "indexerId")?.as_deref())
            .await?),
        "save_pending_nzb" => ok(e.save_pending_nzb(&arg::<String>(a, "token")?, &arg::<String>(a, "dir")?, &arg::<String>(a, "name")?)?),

        // ---- Cola ----
        "get_queue" => ok(e.snapshot()),
        "get_job_files" => ok(e.job_files(&arg::<String>(a, "id")?)),
        "pause_job" => ok(e.pause_job(&arg::<String>(a, "id")?)),
        "resume_job" => ok(e.resume_job(&arg::<String>(a, "id")?)),
        "remove_job" => ok(e.remove_job(&arg::<String>(a, "id")?, arg::<Option<bool>>(a, "deleteFiles")?.unwrap_or(false))),
        "move_job" => ok(e.move_job(&arg::<String>(a, "id")?, arg::<usize>(a, "to")?)),
        "set_job_priority" => ok(e.set_job_priority(&arg::<String>(a, "id")?, arg::<i32>(a, "priority")?)),
        "set_job_category" => ok(e.set_job_category(&arg::<String>(a, "id")?, &arg::<String>(a, "category")?)),
        "set_paused" => ok(e.set_paused(arg::<bool>(a, "paused")?)),
        "set_speed_limit" => ok(e.set_speed_limit(arg::<u64>(a, "kbps")?)?),

        // ---- Historial y estadísticas ----
        "get_history" => ok(e.history()),
        "get_stats" => ok(e.stats()),
        "get_indexer_stats" => ok(e.indexer_stats()),
        "remove_history" => ok(e.remove_history(&arg::<String>(a, "id")?, arg::<Option<bool>>(a, "deleteFiles")?.unwrap_or(false))),
        "clear_history" => ok(e.clear_history(arg::<Option<bool>>(a, "onlyCompleted")?.unwrap_or(false))),
        "retry_history" => ok(e.retry_history(&arg::<String>(a, "id")?)?),
        "retry_extract" => ok(e.retry_extract(&arg::<String>(a, "id")?, arg(a, "password")?)?),

        // ---- Indexadores y debrid ----
        "search_indexer" => ok(e
            .search_indexer(
                &arg::<String>(a, "id")?,
                &arg::<String>(a, "query")?,
                arg::<Option<String>>(a, "category")?.as_deref(),
                arg::<Option<u32>>(a, "offset")?.unwrap_or(0),
                arg(a, "searchId")?,
            )
            .await?),
        "test_indexer" => ok(indexer::test(&arg::<IndexerConfig>(a, "indexer")?).await),
        "test_debrid" => ok(debrid::test(&arg::<DebridConfig>(a, "provider")?).await),
        "debrid_cached" => ok(e.debrid_cached(&arg::<String>(a, "token")?).await?),

        // ---- Pasar los ajustes a otro dispositivo ----
        "transfer_export" => ok(transfer::export_qr(&e.settings.read())?),
        "transfer_preview" => ok(transfer::preview(&arg::<String>(a, "code")?)?),
        "transfer_import" => ok(e.transfer_import(&arg::<String>(a, "code")?)?),
        "transfer_receive_start" => ok(transfer::start_receive().await?),
        "transfer_receive_stop" => ok(transfer::stop_receive()),
        "transfer_send" => {
            let s = e.settings.read().clone();
            ok(transfer::send(&s, &arg::<String>(a, "target")?).await?)
        }

        // ---- Control remoto (este equipo es el "PC" que controla el móvil) ----
        "remote_status" => ok(remote::status()),
        "remote_devices" => ok(remote::devices()),
        "remote_revoke" => ok(remote::revoke(&arg::<String>(a, "id")?)),
        "remote_pair_answer" => ok(remote::answer_pairing(&arg::<String>(a, "req")?, arg::<bool>(a, "ok")?)),
        "remote_pcs" => ok(Vec::<Value>::new()),
        "remote_session" => ok(Value::Null),

        // ---- RSS ----
        "rss_status" => ok(rss::status(e)),
        "rss_check_now" => ok(rss::run(e, true).await?),
        "rss_preview" => ok(rss::preview(e, arg::<RssRule>(a, "rule")?).await?),
        "rss_clear_log" => ok(rss::clear_log()),

        // ---- Acciones al terminar ----
        "finish_test" => ok(e.finish_test(arg::<FinishAction>(a, "action")?).await?),
        "power_status" => ok(nebula_core::finish::power_status(&e.finish)),
        "power_cancel" => ok(e.power_cancel()),
        "power_quick" => ok(e.power_quick(arg::<Option<String>>(a, "mode")?.as_deref())),

        _ => Err(t!("server.unknownCommand", command = cmd)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_args() {
        let a = json!({ "deleteFiles": true, "indexerId": null, "id": "x" });
        assert_eq!(arg::<Option<bool>>(&a, "deleteFiles").unwrap(), Some(true));
        assert_eq!(arg::<Option<String>>(&a, "indexerId").unwrap(), None);
        assert_eq!(arg::<Option<String>>(&a, "missing").unwrap(), None);
        assert!(arg::<String>(&a, "missing").is_err());
        assert_eq!(arg::<String>(&a, "id").unwrap(), "x");
    }
}
