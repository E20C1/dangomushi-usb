use crate::workspace;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_CSS: &str = include_str!("../web/app.css");
const APP_JS: &str = include_str!("../web/app.js");
const MAX_HEADER_BYTES: usize = 16 * 1024;

pub fn serve(port: u16, workspace_root: PathBuf) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let workspace_root = Arc::new(workspace_root);

    println!("DANGOMUSHI {}", env!("CARGO_PKG_VERSION"));
    println!("gateway: http://127.0.0.1:{port}");
    println!(
        "workspace: {}",
        workspace::canonical_or_original(workspace_root.as_ref()).display()
    );
    println!("network: LOOPBACK ONLY");
    println!("press Ctrl+C to stop");

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let workspace_root = Arc::clone(&workspace_root);
                thread::spawn(move || {
                    if let Err(error) = handle_client(stream, port, workspace_root.as_ref()) {
                        eprintln!("gateway request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("gateway accept failed: {error}"),
        }
    }

    Ok(())
}

fn handle_client(mut stream: TcpStream, port: u16, workspace_root: &PathBuf) -> io::Result<()> {
    if !stream.peer_addr()?.ip().is_loopback() {
        return write_response(
            &mut stream,
            403,
            "Forbidden",
            "text/plain; charset=utf-8",
            b"loopback only\n",
            false,
        );
    }

    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;

    let request = read_request_headers(&mut stream)?;
    let request_text = String::from_utf8_lossy(&request);
    let mut lines = request_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();

    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return write_response(
            &mut stream,
            400,
            "Bad Request",
            "text/plain; charset=utf-8",
            b"invalid HTTP version\n",
            method == "HEAD",
        );
    }

    let host = request_text
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .find_map(|(name, value)| {
            name.eq_ignore_ascii_case("host")
                .then_some(value.trim())
        });

    if version == "HTTP/1.1" && !host_is_allowed(host, port) {
        return write_response(
            &mut stream,
            421,
            "Misdirected Request",
            "text/plain; charset=utf-8",
            b"invalid host\n",
            method == "HEAD",
        );
    }

    if !matches!(method, "GET" | "HEAD") {
        return write_response(
            &mut stream,
            405,
            "Method Not Allowed",
            "text/plain; charset=utf-8",
            b"GET and HEAD only\n",
            false,
        );
    }

    let target = raw_target.split('?').next().unwrap_or(raw_target);
    let head_only = method == "HEAD";

    match target {
        "/" | "/index.html" => write_response(
            &mut stream,
            200,
            "OK",
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
            head_only,
        ),
        "/app.css" => write_response(
            &mut stream,
            200,
            "OK",
            "text/css; charset=utf-8",
            APP_CSS.as_bytes(),
            head_only,
        ),
        "/app.js" => write_response(
            &mut stream,
            200,
            "OK",
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
            head_only,
        ),
        "/api/status" => {
            let summary = workspace::summary(workspace_root);
            let path = workspace::canonical_or_original(workspace_root);
            let body = format!(
                "{{\"status\":\"ok\",\"service\":\"dangomushi\",\"version\":\"{}\",\"network\":\"loopback-only\",\"workspace\":{{\"path\":\"{}\",\"initialized\":{},\"presentDirectories\":{},\"totalDirectories\":{}}}}}",
                env!("CARGO_PKG_VERSION"),
                json_escape(&path.to_string_lossy()),
                summary.initialized,
                summary.present_dirs,
                summary.total_dirs
            );
            write_response(
                &mut stream,
                200,
                "OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                head_only,
            )
        }
        "/favicon.ico" => write_response(
            &mut stream,
            204,
            "No Content",
            "image/x-icon",
            b"",
            head_only,
        ),
        _ => write_response(
            &mut stream,
            404,
            "Not Found",
            "text/plain; charset=utf-8",
            b"not found\n",
            head_only,
        ),
    }
}

fn read_request_headers(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut request = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];

    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }

        request.extend_from_slice(&buffer[..read]);

        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }

        if request.len() >= MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request headers exceed limit",
            ));
        }
    }

    Ok(request)
}

fn host_is_allowed(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };

    host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}"))
        || host.eq_ignore_ascii_case(&format!("localhost:{port}"))
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\n\
Content-Type: {content_type}\r\n\
Content-Length: {}\r\n\
Cache-Control: no-store\r\n\
Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\n\
Cross-Origin-Resource-Policy: same-origin\r\n\
Referrer-Policy: no-referrer\r\n\
X-Content-Type-Options: nosniff\r\n\
X-Frame-Options: DENY\r\n\
Connection: close\r\n\r\n",
        body.len()
    );

    stream.write_all(headers.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_validation_is_loopback_only() {
        assert!(host_is_allowed(Some("127.0.0.1:45821"), 45_821));
        assert!(host_is_allowed(Some("LOCALHOST:45821"), 45_821));
        assert!(!host_is_allowed(Some("192.168.1.10:45821"), 45_821));
        assert!(!host_is_allowed(Some("example.com"), 45_821));
        assert!(!host_is_allowed(None, 45_821));
    }

    #[test]
    fn json_escape_handles_windows_paths_and_quotes() {
        assert_eq!(
            json_escape("C:\\USB\\\"workspace\""),
            "C:\\\\USB\\\\\\\"workspace\\\""
        );
    }
}
