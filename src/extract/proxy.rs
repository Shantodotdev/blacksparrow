//! # Guarded Forward Proxy for the Browser
//!
//! Chromium resolves host names itself, so checking a request's URL before letting it go (the
//! CDP `Fetch` hook) cannot stop DNS rebinding: the name may resolve to a public address for the
//! check and to a private one for the connection. Chromium also opens connections the `Fetch`
//! hook never sees (WebSockets, preconnects).
//!
//! The renderer therefore sends all browser traffic through this proxy on the loopback
//! interface. It resolves each target through the [`AddressPolicy`] and connects to exactly the
//! addresses that passed, so every browser connection lands on a checked address. `CONNECT`
//! tunnels carry HTTPS and secure WebSockets; plain HTTP requests in absolute form are forwarded
//! one per connection.
//!
//! With an upstream proxy (an egress proxy the operator configured), connections are relayed to
//! it instead and the upstream resolves names, so the check is advisory there; the upstream must
//! enforce its own egress rules.

use super::guard::{AddressPolicy, GuardRefusal};
use base64::Engine;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use url::Url;

/// Largest request or response head the proxy reads.
const MAX_HEAD_BYTES: usize = 16 * 1024;
/// Longest wait for a client's request head or for a connection to the target.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// A running proxy, stopped when dropped.
pub struct GuardProxy {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl GuardProxy {
    /// Starts the proxy on an ephemeral loopback port.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the port cannot be bound.
    pub async fn start(policy: AddressPolicy, upstream: Option<Url>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let task = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let upstream = upstream.clone();
                tokio::spawn(async move {
                    let _ = serve(client, policy, upstream.as_ref()).await;
                });
            }
        });
        Ok(Self { addr, task })
    }

    /// The proxy's address as Chromium's `--proxy-server` value.
    pub fn server(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for GuardProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A parsed request head from the browser.
struct Head {
    method: String,
    target: String,
    version: String,
    /// Header lines without the request line, hop-by-hop proxy headers removed.
    headers: Vec<String>,
}

async fn serve(
    mut client: TcpStream,
    policy: AddressPolicy,
    upstream: Option<&Url>,
) -> std::io::Result<()> {
    let (raw, rest) = match tokio::time::timeout(HANDSHAKE_TIMEOUT, read_head(&mut client)).await {
        Ok(Ok(Some(read))) => read,
        _ => return Ok(()),
    };
    let Some(head) = parse_head(&raw) else {
        return reply(&mut client, "400 Bad Request").await;
    };
    tracing::trace!("proxy request: {} {}", head.method, head.target);

    if head.method.eq_ignore_ascii_case("CONNECT") {
        let Some((host, port)) = split_authority(&head.target) else {
            return reply(&mut client, "400 Bad Request").await;
        };
        let addrs = match policy.resolve(&host, port).await {
            Ok(addrs) => addrs,
            Err(refusal) => return refuse(&mut client, refusal).await,
        };
        let mut server = match upstream {
            Some(upstream) => {
                let request = format!(
                    "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n{}\r\n",
                    proxy_authorization(upstream)
                );
                match connect_upstream(upstream, request.as_bytes()).await {
                    Some(server) => server,
                    None => return reply(&mut client, "502 Bad Gateway").await,
                }
            }
            None => match connect_any(&addrs).await {
                Some(server) => server,
                None => return reply(&mut client, "502 Bad Gateway").await,
            },
        };
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        if !rest.is_empty() {
            server.write_all(&rest).await?;
        }
        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
        return Ok(());
    }

    // Plain HTTP: the target is an absolute http:// URL.
    let Ok(url) = Url::parse(&head.target) else {
        return reply(&mut client, "400 Bad Request").await;
    };
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        return reply(&mut client, "400 Bad Request").await;
    }
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
        return reply(&mut client, "400 Bad Request").await;
    };
    let addrs = match policy.resolve(host, port).await {
        Ok(addrs) => addrs,
        Err(refusal) => return refuse(&mut client, refusal).await,
    };
    let mut headers = head.headers;
    headers.retain(|line| !header_is(line, "connection"));
    headers.push("Connection: close".to_string());
    let (mut server, request_line) = match upstream {
        Some(upstream) => {
            let auth = proxy_authorization(upstream);
            if !auth.is_empty() {
                headers.push(auth.trim_end().to_string());
            }
            let Some(server) = connect_upstream_raw(upstream).await else {
                return reply(&mut client, "502 Bad Gateway").await;
            };
            (server, format!("{} {} {}", head.method, url, head.version))
        }
        None => {
            let Some(server) = connect_any(&addrs).await else {
                return reply(&mut client, "502 Bad Gateway").await;
            };
            let mut origin = url.path().to_string();
            if let Some(query) = url.query() {
                origin.push('?');
                origin.push_str(query);
            }
            (
                server,
                format!("{} {} {}", head.method, origin, head.version),
            )
        }
    };
    let mut out = request_line;
    out.push_str("\r\n");
    for line in &headers {
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    server.write_all(out.as_bytes()).await?;
    if !rest.is_empty() {
        server.write_all(&rest).await?;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
    Ok(())
}

/// Reads up to the blank line ending a head; returns the head and any bytes read past it.
async fn read_head(stream: &mut TcpStream) -> std::io::Result<Option<(String, Vec<u8>)>> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let rest = buffer.split_off(end + 4);
            return Ok(Some((String::from_utf8_lossy(&buffer).into_owned(), rest)));
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return Ok(None);
        }
    }
}

fn parse_head(raw: &str) -> Option<Head> {
    let mut lines = raw.split("\r\n").filter(|line| !line.is_empty());
    let mut parts = lines.next()?.split_whitespace();
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return None;
    }
    let headers = lines
        .filter(|line| {
            !header_is(line, "proxy-connection") && !header_is(line, "proxy-authorization")
        })
        .map(str::to_string)
        .collect();
    Some(Head {
        method: method.to_string(),
        target: target.to_string(),
        version: version.to_string(),
        headers,
    })
}

fn header_is(line: &str, name: &str) -> bool {
    line.split_once(':')
        .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case(name))
}

/// Splits a `CONNECT` target (`host:port`, `[v6]:port`).
fn split_authority(target: &str) -> Option<(String, u16)> {
    let (host, port) = target.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    if host.is_empty() || port == 0 {
        return None;
    }
    Some((host.to_string(), port))
}

async fn connect_any(addrs: &[SocketAddr]) -> Option<TcpStream> {
    for addr in addrs {
        if let Ok(Ok(stream)) =
            tokio::time::timeout(HANDSHAKE_TIMEOUT, TcpStream::connect(addr)).await
        {
            let _ = stream.set_nodelay(true);
            return Some(stream);
        }
    }
    None
}

async fn connect_upstream_raw(upstream: &Url) -> Option<TcpStream> {
    let host = upstream.host_str()?;
    let port = upstream.port_or_known_default()?;
    let connect = TcpStream::connect((host, port));
    let stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, connect)
        .await
        .ok()?
        .ok()?;
    let _ = stream.set_nodelay(true);
    Some(stream)
}

/// Opens a tunnel through the upstream proxy; `None` unless it answers 2xx.
async fn connect_upstream(upstream: &Url, request: &[u8]) -> Option<TcpStream> {
    let mut stream = connect_upstream_raw(upstream).await?;
    stream.write_all(request).await.ok()?;
    let (head, rest) = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_head(&mut stream))
        .await
        .ok()?
        .ok()??;
    let status_ok = head
        .split_whitespace()
        .nth(1)
        .is_some_and(|code| code.starts_with('2'));
    // A tunnel answer carries no body, so nothing may follow the head yet.
    (status_ok && rest.is_empty()).then_some(stream)
}

/// The `Proxy-Authorization` header line (with CRLF) for an upstream URL with credentials.
fn proxy_authorization(upstream: &Url) -> String {
    if upstream.username().is_empty() {
        return String::new();
    }
    let user = percent_decode(upstream.username());
    let password = percent_decode(upstream.password().unwrap_or(""));
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    format!("Proxy-Authorization: Basic {token}\r\n")
}

fn percent_decode(value: &str) -> String {
    url::form_urlencoded::parse(format!("v={value}").as_bytes())
        .next()
        .map(|(_, decoded)| decoded.into_owned())
        .unwrap_or_default()
}

async fn refuse(client: &mut TcpStream, refusal: GuardRefusal) -> std::io::Result<()> {
    tracing::debug!("proxy refused a browser connection: {}", refusal.as_str());
    reply(client, "403 Forbidden").await
}

async fn reply(client: &mut TcpStream, status: &str) -> std::io::Result<()> {
    client
        .write_all(
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn send(proxy: &GuardProxy, request: &str) -> String {
        let mut stream = TcpStream::connect(proxy.addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut answer = vec![0u8; 512];
        let read = stream.read(&mut answer).await.unwrap();
        String::from_utf8_lossy(&answer[..read]).into_owned()
    }

    #[tokio::test]
    async fn refuses_private_targets() {
        let policy = AddressPolicy {
            allow_private_network: false,
        };
        let proxy = GuardProxy::start(policy, None).await.unwrap();
        let tunnel = send(&proxy, "CONNECT 127.0.0.1:6379 HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(tunnel.starts_with("HTTP/1.1 403"), "{tunnel}");
        let metadata = send(
            &proxy,
            "GET http://169.254.169.254/latest/meta-data HTTP/1.1\r\nHost: 169.254.169.254\r\n\r\n",
        )
        .await;
        assert!(metadata.starts_with("HTTP/1.1 403"), "{metadata}");
        let named = send(&proxy, "CONNECT localhost:443 HTTP/1.1\r\n\r\n").await;
        assert!(named.starts_with("HTTP/1.1 403"), "{named}");
    }

    #[tokio::test]
    async fn forwards_plain_http_in_origin_form() {
        let target = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = target.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = target.accept().await.unwrap();
            let (head, _) = read_head(&mut socket).await.unwrap().unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await
                .unwrap();
            head
        });
        let policy = AddressPolicy {
            allow_private_network: true,
        };
        let proxy = GuardProxy::start(policy, None).await.unwrap();
        let answer = send(
            &proxy,
            &format!(
                "GET http://127.0.0.1:{port}/a?b=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nProxy-Connection: keep-alive\r\n\r\n"
            ),
        )
        .await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        let head = server.await.unwrap();
        assert!(head.starts_with("GET /a?b=1 HTTP/1.1\r\n"), "{head}");
        assert!(!head.to_ascii_lowercase().contains("proxy-connection"));
        assert!(head.contains("Connection: close"));
    }

    #[test]
    fn splits_connect_targets() {
        assert_eq!(
            split_authority("example.com:443"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(split_authority("[::1]:8443"), Some(("[::1]".into(), 8443)));
        assert_eq!(split_authority("example.com"), None);
    }
}
