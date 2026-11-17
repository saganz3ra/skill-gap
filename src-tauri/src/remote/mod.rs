//! Segunda tela no celular: HTTP local + SSE (~1 leitura/s) + QR code.
//!
//! Segurança (o servidor é um processo do usuário escutando na rede local):
//! - token aleatório de 128 bits NOVO a cada início, no caminho (`/<token>/`),
//!   comparado em tempo constante; qualquer outro caminho: 404 sem corpo;
//! - só aceita conexões da rede local (privada, link-local ou loopback);
//! - no máximo 4 conexões ao mesmo tempo; só GET;
//! - sem CORS; CSP restritiva; `no-store`; `nosniff`; `no-referrer`;
//! - o servidor só existe enquanto a segunda tela está ligada.

use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::error::AppResult;

pub const MAX_CONNECTIONS: usize = 4;

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Page,
    Events,
    NotFound,
}

/// Comparação sem atalho: o tempo não revela quantos caracteres batem.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn route(request_line: &str, token: &str) -> Route {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(path)) = (parts.next(), parts.next()) else { return Route::NotFound };
    let Some(rest) = path.strip_prefix('/') else { return Route::NotFound };
    let (t, tail) = rest.split_once('/').unwrap_or((rest, ""));
    if !ct_eq(t, token) {
        return Route::NotFound;
    }
    match tail {
        "" => Route::Page,
        "events" => Route::Events,
        _ => Route::NotFound,
    }
}

pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local(),
        IpAddr::V6(v) => v.is_loopback() || (v.segments()[0] & 0xffc0) == 0xfe80 || (v.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// IP desta máquina na rede local (sem mandar pacote: só pergunta a rota).
pub fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:80").ok()?;
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| is_local(*ip))
}

const HEADERS: &str = "Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'\r\n";

const PAGE: &str = include_str!("page.html");

pub type Snapshot = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub url: String,
    pub port: u16,
}

pub struct RemoteServer {
    stop: Arc<AtomicBool>,
    pub info: RemoteInfo,
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn not_found(mut s: TcpStream) {
    let _ = s.write_all(format!("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n{HEADERS}Connection: close\r\n\r\n").as_bytes());
}

fn handle(stream: TcpStream, peer: SocketAddr, token: &str, snapshot: &Snapshot, stop: &AtomicBool) {
    if !is_local(peer.ip()) {
        return not_found(stream);
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.len() > 2048 {
        return not_found(stream);
    }
    let mut s = stream;
    match route(&line, token) {
        Route::NotFound => not_found(s),
        Route::Page => {
            let body = PAGE;
            let _ = s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n{HEADERS}Connection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
        Route::Events => {
            if s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n{HEADERS}Connection: keep-alive\r\n\r\n").as_bytes()).is_err() {
                return;
            }
            while !stop.load(Ordering::SeqCst) {
                let data = snapshot().to_string();
                if s.write_all(format!("data: {data}\n\n").as_bytes()).and_then(|_| s.flush()).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

/// Liga o servidor numa porta livre. `token`: 32 hex aleatórios do gerador do sistema.
pub fn start(token: String, snapshot: Snapshot) -> AppResult<RemoteServer> {
    let listener = TcpListener::bind("0.0.0.0:0")?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let host = lan_ip().map(|ip| ip.to_string()).unwrap_or_else(|| "127.0.0.1".into());
    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let flag = Arc::clone(&stop);
    let tok = token.clone();
    std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, peer)) => {
                    let _ = stream.set_nonblocking(false);
                    if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                        not_found(stream);
                        continue;
                    }
                    active.fetch_add(1, Ordering::SeqCst);
                    let (tok, snap, stop, active) = (tok.clone(), Arc::clone(&snapshot), Arc::clone(&flag), Arc::clone(&active));
                    std::thread::spawn(move || {
                        handle(stream, peer, &tok, &snap, &stop);
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(_) => std::thread::sleep(Duration::from_millis(150)),
            }
        }
    });
    Ok(RemoteServer { stop, info: RemoteInfo { url: format!("http://{host}:{port}/{token}/"), port } })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn rotas_so_com_token() {
        assert_eq!(route(&format!("GET /{T}/ HTTP/1.1"), T), Route::Page);
        assert_eq!(route(&format!("GET /{T}/events HTTP/1.1"), T), Route::Events);
        assert_eq!(route("GET / HTTP/1.1", T), Route::NotFound);
        assert_eq!(route(&format!("GET /{T}x/ HTTP/1.1"), T), Route::NotFound);
        assert_eq!(route(&format!("POST /{T}/ HTTP/1.1"), T), Route::NotFound);
        assert_eq!(route(&format!("GET /{T}/../etc HTTP/1.1"), T), Route::NotFound);
        assert!(ct_eq(T, T) && !ct_eq(T, "x") && !ct_eq(T, &T.replace('0', "1")));
    }

    #[test]
    fn so_rede_local() {
        assert!(is_local("192.168.1.20".parse().unwrap()));
        assert!(is_local("10.0.0.5".parse().unwrap()));
        assert!(is_local("fe80::1".parse().unwrap()));
        assert!(!is_local("8.8.8.8".parse().unwrap()));
        assert!(!is_local("2804:14c::1".parse().unwrap()));
    }

    #[test]
    fn servidor_responde_404_sem_token_e_200_com() {
        let srv = start(T.into(), Arc::new(|| serde_json::json!({ "ok": true }))).unwrap();
        let get = |path: &str| {
            let mut s = TcpStream::connect(("127.0.0.1", srv.info.port)).unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
            let mut first = String::new();
            BufReader::new(s).read_line(&mut first).unwrap();
            first
        };
        assert!(get("/").contains("404"));
        assert!(get(&format!("/{T}/")).contains("200"));
        drop(srv);
    }
}
