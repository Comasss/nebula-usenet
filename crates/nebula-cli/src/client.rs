//! Cliente de la API de nebula-server: `POST /api/v1/invoke/<comando>` con la API key en
//! `X-Api-Key`, los mismos comandos que usa la interfaz web.
use crate::{Fail, Res};
use serde_json::Value;

#[derive(Clone)]
pub struct Client {
    pub base: String,
    key: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base: &str, key: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("nebula-cli/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("http client");
        Client { base: base.trim_end_matches('/').to_string(), key: key.to_string(), http }
    }

    async fn send(&self, cmd: &str, req: reqwest::RequestBuilder) -> Res<Value> {
        let res = req.header("x-api-key", &self.key).send().await.map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                Fail::new(t!("cli.unreachable", url = self.base))
            } else {
                Fail::new(e.to_string())
            }
        })?;
        let status = res.status();
        let body = res.bytes().await.map_err(|e| Fail::new(e.to_string()))?;
        let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Fail::new(t!("cli.badKey")));
        }
        if status == reqwest::StatusCode::NOT_FOUND && v.is_null() {
            return Err(Fail::new(t!("cli.notNebula", url = self.base)));
        }
        if !status.is_success() {
            let msg = v.get("error").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{} ({})", status, cmd));
            return Err(Fail::new(msg));
        }
        Ok(v)
    }

    /// Ejecuta un comando de la interfaz con sus argumentos (camelCase).
    pub async fn invoke(&self, cmd: &str, args: Value) -> Res<Value> {
        let req = self
            .http
            .post(format!("{}/api/v1/invoke/{}", self.base, cmd))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&args).unwrap_or_default());
        self.send(cmd, req).await
    }

    /// Sube un .nzb al servidor y devuelve su ruta allí.
    pub async fn import(&self, name: &str, data: Vec<u8>) -> Res<String> {
        let req = self
            .http
            .post(format!("{}/api/v1/invoke/import_nzb", self.base))
            .header("x-name", encode(name))
            .header("content-type", "application/octet-stream")
            .body(data);
        let v = self.send("import_nzb", req).await?;
        v.as_str().map(str::to_string).ok_or_else(|| Fail::new("import_nzb"))
    }
}

/// Como `encodeURIComponent`: el nombre del archivo va en una cabecera.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_names() {
        assert_eq!(super::encode("Show S01 ñ.nzb"), "Show%20S01%20%C3%B1.nzb");
        assert_eq!(nebula_core::util::percent_decode(&super::encode("a b/ñ.nzb")), "a b/ñ.nzb");
    }
}
