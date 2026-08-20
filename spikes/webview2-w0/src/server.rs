//! A loopback origin of our own.
//!
//! The preview block's whole reason for existing is `http://localhost:port`, and
//! several gates need things only a real server can produce: a 302 that lands
//! somewhere the address bar never saw, a `Content-Disposition` that starts a
//! download, a second origin so the site-isolation process count means anything.
//! Serving them from a thread inside the probe keeps the experiment offline and
//! reproducible — nothing here reaches the network.

use anyhow::{Context as _, Result};
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};

pub const PROBE_PAGE: &str = include_str!("../assets/probe.html");
pub const VIDEO_PAGE: &str = include_str!("../assets/video.html");

/// A page whose *script* starts top-level navigations, so `NavigationStarting`
/// is the only door left that can stop them.
const NAVIGATOR_PAGE: &str = r#"<!doctype html>
<meta charset="utf-8"><title>W0 navigator</title>
<body style="background:#25324a;color:#fff;font:13px system-ui;padding:20px">
<h1>navigator</h1>
<script>
  const post = (payload) => window.chrome?.webview?.postMessage(payload);
  post({ kind: 'ready', page: 'navigator' });
  window.chrome?.webview?.addEventListener('message', (event) => {
    const target = event.data;
    if (typeof target === 'string' && target.startsWith('go:')) {
      const url = target.slice(3);
      post({ kind: 'navigating', url });
      // A page script driving the top-level location. The address bar never saw
      // this string; only NavigationStarting can refuse it.
      location.href = url;
    }
  });
</script>
</body>"#;

pub struct Server {
    pub port: u16,
}

impl Server {
    /// Bind on loopback and serve until the process exits.
    pub fn start() -> Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .context("bind loopback listener")?;
        let port = listener.local_addr().context("local_addr")?.port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = serve(stream);
            }
        });
        Ok(Self { port })
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://localhost:{}{path}", self.port)
    }

    /// The same server reached by a name that is a *different origin* to the
    /// browser even though it is the same socket — which is what makes the
    /// site-isolation process count in gate 8 mean something.
    pub fn other_origin_url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

fn serve(mut stream: TcpStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // Drain the headers; nothing here reads them, but leaving them unread makes
    // the client see a reset instead of a response.
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let (path, query) = match path.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (path, String::new()),
    };
    match path.as_str() {
        "/" | "/index.html" => html(&mut stream, PROBE_PAGE),
        "/video" => html(&mut stream, VIDEO_PAGE),
        "/navigator" => html(&mut stream, NAVIGATOR_PAGE),
        "/redirect" => {
            let target = query
                .strip_prefix("to=")
                .map(percent_decode)
                .unwrap_or_else(|| "/".to_owned());
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
            Ok(())
        }
        "/download" => {
            let body = "this is a file the browser is meant to want to save";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                 Content-Disposition: attachment; filename=\"w0-probe.bin\"\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            Ok(())
        }
        "/second" => html(
            &mut stream,
            r#"<!doctype html><meta charset="utf-8"><title>W0 second origin</title>
<body style="background:#3a2140;color:#fff;font:13px system-ui;padding:20px">second origin</body>"#,
        ),
        _ => {
            let body = "not found";
            write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            Ok(())
        }
    }
}

fn html(stream: &mut TcpStream, body: &str) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
         Cache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a URL so it survives being a query parameter.
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
