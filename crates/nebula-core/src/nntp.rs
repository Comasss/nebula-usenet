use crate::config::ServerConfig;
use crate::yenc::YencDecoder;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::{timeout, Sleep};

#[derive(Debug, Clone)]
pub enum NntpError {
    Io(String),
    Timeout,
    Tls(String),
    Auth(String),
    TooMany(String),
    NotFound,
    Unavailable(String),
    Protocol(String),
}

impl NntpError {
    pub fn message(&self) -> String {
        match self {
            NntpError::Io(e) => t!("err.connection", error = e),
            NntpError::Timeout => t!("nntp.timeout"),
            NntpError::Tls(e) => t!("nntp.tls", error = e),
            NntpError::Auth(e) => t!("nntp.auth", detail = e),
            NntpError::TooMany(e) => t!("nntp.tooMany", detail = e),
            NntpError::NotFound => t!("nntp.notFound"),
            NntpError::Unavailable(e) => t!("nntp.unavailable", detail = e),
            NntpError::Protocol(e) => t!("nntp.protocol", detail = e),
        }
    }
}

fn io_err(e: io::Error) -> NntpError {
    if e.kind() == io::ErrorKind::TimedOut {
        return NntpError::Timeout;
    }
    let msg = match e.kind() {
        io::ErrorKind::ConnectionRefused => t!("nntp.refused"),
        io::ErrorKind::ConnectionReset => t!("nntp.closed"),
        io::ErrorKind::UnexpectedEof => t!("nntp.closed"),
        _ => e.to_string(),
    };
    NntpError::Io(msg)
}

fn is_too_many(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("too many") || m.contains("connection limit") || m.contains("max connections") || (m.contains("connections") && (m.contains("limit") || m.contains("maximum") || m.contains("exceed")))
}

enum Inner {
    Plain(TcpStream),
    Tls(tokio_native_tls::TlsStream<TcpStream>),
}

/// Stream con timeout de inactividad: si no llega nada en `idle`, la lectura falla.
struct TimedStream {
    inner: Inner,
    sleep: Pin<Box<Sleep>>,
    idle: Duration,
}

impl TimedStream {
    fn bump(&mut self) {
        let deadline = tokio::time::Instant::from_std(Instant::now() + self.idle);
        self.sleep.as_mut().reset(deadline);
    }
}

impl AsyncRead for TimedStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let r = match &mut this.inner {
            Inner::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Inner::Tls(s) => Pin::new(s).poll_read(cx, buf),
        };
        match r {
            Poll::Ready(res) => {
                this.bump();
                Poll::Ready(res)
            }
            Poll::Pending => {
                if this.sleep.as_mut().poll(cx).is_ready() {
                    Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "timeout")))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

impl AsyncWrite for TimedStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.bump();
        match &mut this.inner {
            Inner::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Inner::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            Inner::Plain(s) => Pin::new(s).poll_flush(cx),
            Inner::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            Inner::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Inner::Tls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

pub struct NntpConn {
    r: BufReader<TimedStream>,
    line: Vec<u8>,
    user: String,
    pass: String,
    pub greeting: String,
}

impl NntpConn {
    pub async fn connect(cfg: &ServerConfig, idle: Duration) -> Result<Self, NntpError> {
        let host = cfg.host.trim();
        if host.is_empty() {
            return Err(NntpError::Io(t!("nntp.noHost")));
        }
        let tcp = connect_tcp(host, cfg.port).await?;
        let _ = tcp.set_nodelay(true);
        let inner = if cfg.ssl {
            let mut b = native_tls::TlsConnector::builder();
            if !cfg.verify_cert {
                b.danger_accept_invalid_certs(true).danger_accept_invalid_hostnames(true);
            }
            let connector = tokio_native_tls::TlsConnector::from(b.build().map_err(|e| NntpError::Tls(e.to_string()))?);
            let tls = timeout(Duration::from_secs(20), connector.connect(host, tcp))
                .await
                .map_err(|_| NntpError::Timeout)?
                .map_err(|e| NntpError::Tls(e.to_string()))?;
            Inner::Tls(tls)
        } else {
            Inner::Plain(tcp)
        };
        let stream = TimedStream {
            inner,
            sleep: Box::pin(tokio::time::sleep(idle)),
            idle,
        };
        let mut conn = NntpConn {
            r: BufReader::with_capacity(256 * 1024, stream),
            line: Vec::with_capacity(16 * 1024),
            user: cfg.username.clone(),
            pass: cfg.password.clone(),
            greeting: String::new(),
        };
        conn.r.get_mut().bump();
        let (code, msg) = conn.read_status().await?;
        conn.greeting = msg.clone();
        match code {
            200 | 201 => {}
            400 | 502 => {
                return Err(if is_too_many(&msg) { NntpError::TooMany(msg) } else { NntpError::Unavailable(msg) })
            }
            _ => return Err(NntpError::Protocol(format!("{} {}", code, msg))),
        }
        if !conn.user.is_empty() {
            conn.auth().await?;
        }
        Ok(conn)
    }

    async fn auth(&mut self) -> Result<(), NntpError> {
        let cmd = format!("AUTHINFO USER {}\r\n", self.user);
        let (code, msg) = self.command(&cmd).await?;
        match code {
            281 => return Ok(()),
            381 => {}
            _ => return Err(classify_auth(code, msg)),
        }
        let cmd = format!("AUTHINFO PASS {}\r\n", self.pass);
        let (code, msg) = self.command(&cmd).await?;
        match code {
            281 => Ok(()),
            _ => Err(classify_auth(code, msg)),
        }
    }

    async fn command(&mut self, cmd: &str) -> Result<(u16, String), NntpError> {
        let w = self.r.get_mut();
        w.write_all(cmd.as_bytes()).await.map_err(io_err)?;
        w.flush().await.map_err(io_err)?;
        self.read_status().await
    }

    async fn read_line(&mut self) -> Result<usize, NntpError> {
        self.line.clear();
        let n = self.r.read_until(b'\n', &mut self.line).await.map_err(io_err)?;
        if n == 0 {
            return Err(NntpError::Io(t!("nntp.closed")));
        }
        Ok(n)
    }

    async fn read_status(&mut self) -> Result<(u16, String), NntpError> {
        self.read_line().await?;
        let s = String::from_utf8_lossy(&self.line).trim().to_string();
        let code: u16 = s
            .get(..3)
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| NntpError::Protocol(t!("nntp.badResponse", detail = s.chars().take(80).collect::<String>())))?;
        Ok((code, s.get(4..).unwrap_or("").to_string()))
    }

    /// Descarga el cuerpo de un artículo y lo decodifica (yEnc). Devuelve los bytes recibidos.
    pub async fn body(&mut self, msgid: &str, dec: &mut YencDecoder) -> Result<u64, NntpError> {
        let cmd = format!("BODY <{}>\r\n", msgid);
        let (mut code, mut msg) = self.command(&cmd).await?;
        if code == 480 && !self.user.is_empty() {
            self.auth().await?;
            (code, msg) = self.command(&cmd).await?;
        }
        match code {
            222 => {}
            430 | 423 | 420 => return Err(NntpError::NotFound),
            480 | 481 | 482 => return Err(classify_auth(code, msg)),
            400 | 502 | 503 => {
                return Err(if is_too_many(&msg) { NntpError::TooMany(msg) } else { NntpError::Unavailable(msg) })
            }
            _ => return Err(NntpError::Protocol(format!("{} {}", code, msg))),
        }
        let mut raw = 0u64;
        loop {
            let n = self.read_line().await?;
            raw += n as u64;
            let mut l: &[u8] = &self.line;
            while let Some((&last, rest)) = l.split_last() {
                if last == b'\n' || last == b'\r' {
                    l = rest;
                } else {
                    break;
                }
            }
            if l == b"." {
                break;
            }
            if l.starts_with(b"..") {
                l = &l[1..];
            }
            dec.feed_line(l);
        }
        Ok(raw)
    }

    pub async fn date(&mut self) -> Result<String, NntpError> {
        let (code, msg) = self.command("DATE\r\n").await?;
        if code == 111 {
            Ok(msg)
        } else {
            Err(NntpError::Protocol(format!("{} {}", code, msg)))
        }
    }

    pub async fn quit(mut self) {
        let _ = timeout(Duration::from_secs(2), self.command("QUIT\r\n")).await;
    }
}

/// Conecta por TCP probando todas las direcciones del host (IPv4 primero) de forma escalonada:
/// si una ruta (típicamente IPv6 tras una VPN) no responde, no bloquea a las demás.
async fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, NntpError> {
    let not_found = || NntpError::Io(t!("nntp.hostNotFound", host = host));
    let addrs: Vec<std::net::SocketAddr> = timeout(Duration::from_secs(10), tokio::net::lookup_host((host, port)))
        .await
        .map_err(|_| NntpError::Io(t!("nntp.dns", host = host)))?
        .map_err(|_| not_found())?
        .collect();
    let (v4, v6): (Vec<_>, Vec<_>) = addrs.into_iter().partition(|a| a.is_ipv4());
    let mut ordered: Vec<std::net::SocketAddr> = Vec::with_capacity(v4.len() + v6.len());
    for i in 0..v4.len().max(v6.len()) {
        ordered.extend(v4.get(i).copied());
        ordered.extend(v6.get(i).copied());
    }
    if ordered.is_empty() {
        return Err(not_found());
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut set = tokio::task::JoinSet::new();
    let mut next = 0;
    let mut last_err: Option<NntpError> = None;
    loop {
        if next < ordered.len() {
            let a = ordered[next];
            next += 1;
            set.spawn(async move { timeout(Duration::from_secs(15), TcpStream::connect(a)).await });
        }
        let wait = if next < ordered.len() {
            Duration::from_millis(250)
        } else {
            deadline.saturating_duration_since(tokio::time::Instant::now())
        };
        match timeout(wait, set.join_next()).await {
            Ok(Some(Ok(Ok(Ok(stream))))) => {
                set.abort_all();
                return Ok(stream);
            }
            Ok(Some(Ok(Ok(Err(e))))) => last_err = Some(io_err(e)),
            Ok(Some(Ok(Err(_)))) => last_err = Some(NntpError::Timeout),
            Ok(Some(Err(_))) => {}
            Ok(None) => {
                if next >= ordered.len() {
                    break;
                }
            }
            Err(_) => {
                if next >= ordered.len() {
                    break;
                }
            }
        }
    }
    set.abort_all();
    Err(match last_err {
        Some(NntpError::Timeout) | None => NntpError::Io(t!("nntp.noResponse", host = host, port = port)),
        Some(e) => e,
    })
}

fn classify_auth(code: u16, msg: String) -> NntpError {
    if is_too_many(&msg) {
        NntpError::TooMany(msg)
    } else {
        NntpError::Auth(format!("{} {}", code, msg).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sondeo real sin credenciales: NEBULA_PROBE=host:puerto[:ssl] cargo test probe -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn probe() {
        let spec = std::env::var("NEBULA_PROBE").expect("NEBULA_PROBE=host:puerto[:ssl]");
        let parts: Vec<&str> = spec.split(':').collect();
        let cfg = ServerConfig {
            host: parts[0].into(),
            port: parts[1].parse().unwrap(),
            ssl: parts.get(2) == Some(&"ssl"),
            username: String::new(),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        match NntpConn::connect(&cfg, Duration::from_secs(20)).await {
            Ok(c) => println!("OK en {} ms · saludo: {}", t.elapsed().as_millis(), c.greeting),
            Err(e) => panic!("fallo tras {} ms: {}", t.elapsed().as_millis(), e.message()),
        }
    }
}
