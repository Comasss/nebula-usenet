//! Servidor NNTP simulado para medir el rendimiento (dev/bench.py).
//!
//! Lee todos los archivos de una carpeta, los parte en artículos yEnc de `--part` bytes, los
//! guarda codificados en memoria y escribe el .nzb. Después atiende `AUTHINFO`, `STAT`, `BODY`,
//! `ARTICLE`, `GROUP`, `DATE` y `QUIT` en `--port`, sin límite de velocidad, de forma que el
//! cliente sea el cuello de botella.
//!
//! Uso: nntp-bench --src CARPETA --nzb salida.nzb [--port 1119] [--part 768000]
//!                 [--max-conns 100] [--missing 0] [--user test --password test]
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpListener;

const LINE: usize = 128;

struct Args {
    src: PathBuf,
    nzb: PathBuf,
    port: u16,
    part: usize,
    max_conns: usize,
    missing: usize,
    user: String,
    password: String,
}

fn args() -> Args {
    let mut a = Args { src: PathBuf::new(), nzb: PathBuf::new(), port: 1119, part: 768_000, max_conns: 100, missing: 0, user: "test".into(), password: "test".into() };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let v = it.next().unwrap_or_else(|| panic!("falta el valor de {k}"));
        match k.as_str() {
            "--src" => a.src = v.into(),
            "--nzb" => a.nzb = v.into(),
            "--port" => a.port = v.parse().expect("--port"),
            "--part" => a.part = v.parse().expect("--part"),
            "--max-conns" => a.max_conns = v.parse().expect("--max-conns"),
            "--missing" => a.missing = v.parse().expect("--missing"),
            "--user" => a.user = v,
            "--password" => a.password = v,
            _ => panic!("opción desconocida: {k}"),
        }
    }
    assert!(!a.src.as_os_str().is_empty() && !a.nzb.as_os_str().is_empty(), "uso: nntp-bench --src CARPETA --nzb salida.nzb");
    a
}

/// Codifica un trozo en líneas yEnc con dot-stuffing, terminadas en CRLF.
fn yenc(data: &[u8], out: &mut Vec<u8>) {
    let mut col = 0;
    for &b in data {
        let c = b.wrapping_add(42);
        let esc = matches!(c, 0 | b'\n' | b'\r' | b'=') || (col == 0 && c == b'.');
        if col == 0 && c == b'.' {
            out.push(b'.'); // dot-stuffing
        }
        if esc && c != b'.' {
            out.push(b'=');
            out.push(c.wrapping_add(64));
            col += 2;
        } else {
            out.push(c);
            col += 1;
        }
        if col >= LINE {
            out.extend_from_slice(b"\r\n");
            col = 0;
        }
    }
    if col > 0 {
        out.extend_from_slice(b"\r\n");
    }
}

struct Segment {
    number: usize,
    bytes: usize,
    id: String,
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Pseudoaleatorio simple y reproducible para elegir los artículos que faltan.
fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

fn build(a: &Args) -> (HashMap<String, Arc<[u8]>>, HashSet<String>) {
    let mut names: Vec<_> = std::fs::read_dir(&a.src).expect("--src").filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let mut store = HashMap::new();
    let mut files: Vec<(String, Vec<Segment>)> = Vec::new();
    let mut data_ids = Vec::new();
    for name in &names {
        let data = std::fs::read(a.src.join(name)).expect("leer");
        let total = data.len().div_ceil(a.part).max(1);
        // Codificar en paralelo: con varios GB tarda menos.
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chunks: Vec<(usize, &[u8])> = data.chunks(a.part).enumerate().collect();
        let size = data.len();
        let mut bodies: Vec<(usize, Vec<u8>)> = std::thread::scope(|s| {
            let per = chunks.len().div_ceil(threads).max(1);
            let hs: Vec<_> = chunks
                .chunks(per)
                .map(|group| {
                    s.spawn(move || {
                        group
                            .iter()
                            .map(|&(i, chunk)| {
                                let begin = i * a.part + 1;
                                let mut b = Vec::with_capacity(chunk.len() * 103 / 100 + 256);
                                let _ = write!(Str(&mut b), "=ybegin part={} total={} line={} size={} name={}\r\n=ypart begin={} end={}\r\n", i + 1, total, LINE, size, name, begin, begin + chunk.len() - 1);
                                yenc(chunk, &mut b);
                                let _ = write!(Str(&mut b), "=yend size={} part={} pcrc32={:08x}\r\n.\r\n", chunk.len(), i + 1, crc32fast::hash(chunk));
                                (i, b)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        bodies.sort_by_key(|b| b.0);
        let mut segs = Vec::new();
        for (i, body) in bodies {
            let id = format!("{}.{}.{}@bench.nebula", name.replace(' ', "_"), i + 1, data.len());
            segs.push(Segment { number: i + 1, bytes: body.len(), id: id.clone() });
            if !name.to_ascii_lowercase().ends_with(".par2") {
                data_ids.push(id.clone());
            }
            store.insert(id, Arc::from(body));
        }
        eprintln!("  {}: {} bytes, {} artículos", name, data.len(), total);
        files.push((name.clone(), segs));
    }
    let mut missing = HashSet::new();
    let mut seed = 7u64;
    while missing.len() < a.missing.min(data_ids.len()) {
        missing.insert(data_ids[(lcg(&mut seed) as usize) % data_ids.len()].clone());
    }
    let mut nzb = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n");
    for (idx, (name, segs)) in files.iter().enumerate() {
        let _ = writeln!(nzb, " <file poster=\"bench@nebula\" date=\"1700000000\" subject=\"{}\">", xml(&format!("[{}/{}] - \"{}\" yEnc (1/{})", idx + 1, files.len(), name, segs.len())));
        nzb.push_str("  <groups><group>alt.binaries.test</group></groups>\n  <segments>\n");
        for s in segs {
            let _ = writeln!(nzb, "   <segment bytes=\"{}\" number=\"{}\">{}</segment>", s.bytes, s.number, xml(&s.id));
        }
        nzb.push_str("  </segments>\n </file>\n");
    }
    nzb.push_str("</nzb>\n");
    std::fs::write(&a.nzb, nzb).expect("escribir el NZB");
    (store, missing)
}

/// `write!` sobre un Vec<u8>.
struct Str<'a>(&'a mut Vec<u8>);
impl std::fmt::Write for Str<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let a = args();
    let (store, missing) = build(&a);
    let store = Arc::new(store);
    let missing = Arc::new(missing);
    let listener = TcpListener::bind(("127.0.0.1", a.port)).await.expect("puerto ocupado");
    let active = Arc::new(AtomicUsize::new(0));
    let sent = Arc::new(AtomicU64::new(0));
    let bodies = Arc::new(AtomicU64::new(0));
    let (user, password, max) = (Arc::new(a.user), Arc::new(a.password), a.max_conns);
    // dev/bench.py espera esta línea para empezar.
    println!("READY {} articles on 127.0.0.1:{}", store.len(), a.port);
    let start = std::time::Instant::now();
    // Milisegundos desde READY y bytes enviados, para que dev/bench.py mida la velocidad por el
    // lado del servidor (con la hora del servidor, que no se retrasa si el script va cargado).
    {
        let sent = sent.clone();
        tokio::spawn(async move {
            let mut last = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let now = sent.load(Ordering::Relaxed);
                if now != last {
                    last = now;
                    println!("SENT {} {now}", start.elapsed().as_millis());
                }
            }
        });
    }
    {
        let (sent, bodies) = (sent.clone(), bodies.clone());
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            eprintln!("{} BODY, {} MB enviados", bodies.load(Ordering::Relaxed), sent.load(Ordering::Relaxed) / 1_000_000);
            std::process::exit(0);
        });
    }
    loop {
        let Ok((sock, _)) = listener.accept().await else { continue };
        let _ = sock.set_nodelay(true);
        let (store, missing, active, sent, bodies, user, password) = (store.clone(), missing.clone(), active.clone(), sent.clone(), bodies.clone(), user.clone(), password.clone());
        tokio::spawn(async move {
            let (r, w) = sock.into_split();
            let mut r = BufReader::new(r);
            let mut w = BufWriter::with_capacity(64 * 1024, w);
            if active.fetch_add(1, Ordering::SeqCst) >= max {
                let _ = w.write_all(b"502 Too many connections\r\n").await;
                let _ = w.flush().await;
                active.fetch_sub(1, Ordering::SeqCst);
                return;
            }
            let _ = w.write_all(b"200 nntp-bench ready\r\n").await;
            let _ = w.flush().await;
            let (mut authed, mut got_user) = (false, String::new());
            let mut line = String::new();
            loop {
                line.clear();
                match r.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    _ => {}
                }
                let cmd = line.trim();
                let up = cmd.to_ascii_uppercase();
                let arg = |n: usize| cmd.get(n..).unwrap_or("").trim().trim_matches(|c| c == '<' || c == '>').to_string();
                let res: std::io::Result<()> = async {
                    if up.starts_with("AUTHINFO USER") {
                        got_user = arg(13);
                        w.write_all(b"381 Password required\r\n").await
                    } else if up.starts_with("AUTHINFO PASS") {
                        authed = got_user == *user && arg(13) == *password;
                        w.write_all(if authed { b"281 Authentication accepted\r\n" as &[u8] } else { b"481 Authentication failed\r\n" }).await
                    } else if up.starts_with("QUIT") {
                        w.write_all(b"205 Bye\r\n").await
                    } else if !authed {
                        w.write_all(b"480 Authentication required\r\n").await
                    } else if up.starts_with("STAT") {
                        let id = arg(4);
                        let ok = store.contains_key(&id) && !missing.contains(&id);
                        w.write_all(if ok { format!("223 0 <{id}>\r\n") } else { "430 No such article\r\n".into() }.as_bytes()).await
                    } else if up.starts_with("BODY") || up.starts_with("ARTICLE") {
                        let art = up.starts_with("ARTICLE");
                        let id = arg(if art { 7 } else { 4 });
                        match store.get(&id).filter(|_| !missing.contains(&id)) {
                            Some(body) => {
                                bodies.fetch_add(1, Ordering::Relaxed);
                                sent.fetch_add(body.len() as u64, Ordering::Relaxed);
                                if art {
                                    w.write_all(format!("220 0 <{id}>\r\nMessage-ID: <{id}>\r\nNewsgroups: alt.binaries.test\r\n\r\n").as_bytes()).await?;
                                } else {
                                    w.write_all(format!("222 0 <{id}>\r\n").as_bytes()).await?;
                                }
                                w.write_all(body).await
                            }
                            None => w.write_all(b"430 No such article\r\n").await,
                        }
                    } else if up.starts_with("GROUP") {
                        w.write_all(format!("211 {} 1 {} alt.binaries.test\r\n", store.len(), store.len()).as_bytes()).await
                    } else if up.starts_with("DATE") {
                        w.write_all(b"111 20260101000000\r\n").await
                    } else if up.starts_with("CAPABILITIES") {
                        w.write_all(b"101 Capability list:\r\nVERSION 2\r\nREADER\r\nAUTHINFO USER\r\n.\r\n").await
                    } else if up.starts_with("MODE READER") {
                        w.write_all(b"200 Reader mode\r\n").await
                    } else {
                        w.write_all(b"500 Unknown command\r\n").await
                    }
                }
                .await;
                if res.is_err() || w.flush().await.is_err() || up.starts_with("QUIT") {
                    break;
                }
            }
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_nebula_decoder_rules() {
        // Todos los bytes, con '.' al principio de línea y caracteres críticos.
        let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let mut enc = Vec::new();
        yenc(&data, &mut enc);
        let mut out = Vec::new();
        for l in enc.split(|&b| b == b'\n') {
            let l = l.strip_suffix(b"\r").unwrap_or(l);
            let l = l.strip_prefix(b"..").map(|r| [b".".as_slice(), r].concat()).unwrap_or(l.to_vec());
            let mut i = 0;
            while i < l.len() {
                if l[i] == b'=' {
                    i += 1;
                    out.push(l[i].wrapping_sub(106));
                } else {
                    out.push(l[i].wrapping_sub(42));
                }
                i += 1;
            }
        }
        assert_eq!(out, data);
        assert!(enc.split(|&b| b == b'\n').all(|l| l.len() <= LINE + 3));
    }
}
