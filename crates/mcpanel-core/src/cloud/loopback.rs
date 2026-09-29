//! The loopback redirect receiver (RFC 8252 §7.3): a tiny HTTP listener on the loopback
//! interface that waits for the browser to deliver `?code=…&state=…` (or `?error=…`).
//! It only answers GET requests on the expected path and never serves anything else.

use crate::error::{CoreError, CoreResult, ErrorCode};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the browser delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    Code {
        code: String,
        state: String,
    },
    Error {
        error: String,
        description: Option<String>,
        state: Option<String>,
    },
}

/// Listeners bound to one port on the loopback interface(s).
pub struct Loopback {
    listeners: Vec<TcpListener>,
    pub port: u16,
    path: String,
}

impl Loopback {
    /// Bind `port` (0 = any free port) on 127.0.0.1, and also on ::1 when the redirect
    /// uses `localhost` (browsers may resolve it to either).
    pub async fn bind(host: &str, port: u16, path: &str) -> CoreResult<Self> {
        let v4 = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
            CoreError::new(
                ErrorCode::PortInUse,
                format!("Cannot listen on 127.0.0.1:{port} for the sign-in redirect: {e}"),
            )
        })?;
        let port = v4
            .local_addr()
            .map_err(|e| CoreError::io("Cannot read the listener address", &e))?
            .port();
        let mut listeners = vec![v4];
        if host == "localhost"
            && let Ok(v6) = TcpListener::bind(("::1", port)).await
        {
            listeners.push(v6);
        }
        Ok(Self {
            listeners,
            port,
            path: path.to_string(),
        })
    }

    /// Wait for the redirect (at most `timeout`), answering the browser with a short page.
    pub async fn wait(self, timeout: Duration) -> CoreResult<Callback> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let accept = async {
                if self.listeners.len() == 1 {
                    self.listeners[0].accept().await
                } else {
                    tokio::select! {
                        r = self.listeners[0].accept() => r,
                        r = self.listeners[1].accept() => r,
                    }
                }
            };
            let (stream, _) = match tokio::time::timeout_at(deadline, accept).await {
                Err(_) => {
                    return Err(CoreError::new(
                        ErrorCode::Cancelled,
                        "The sign-in was not completed in time",
                    ));
                }
                Ok(Err(e)) => return Err(CoreError::io("Sign-in listener failed", &e)),
                Ok(Ok(s)) => s,
            };
            if let Some(cb) = self.handle(stream).await {
                return Ok(cb);
            }
        }
    }

    /// Read one request; `None` for anything that is not the redirect (e.g. favicon).
    async fn handle(&self, mut stream: TcpStream) -> Option<Callback> {
        let mut buf = vec![0u8; 16 * 1024];
        let mut n = 0;
        let read = async {
            while n < buf.len() {
                let r = stream.read(&mut buf[n..]).await.ok()?;
                if r == 0 {
                    break;
                }
                n += r;
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Some(())
        };
        tokio::time::timeout(Duration::from_secs(5), read)
            .await
            .ok()??;
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        let target = head
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("GET "))
            .and_then(|l| l.split(' ').next())?
            .to_string();
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        if path != self.path {
            let _ = respond(&mut stream, 404, "Not found").await;
            return None;
        }
        let cb = parse_query(query);
        let page = match &cb {
            Some(Callback::Code { .. }) => {
                "MCPanel is connected. You can close this window and return to MCPanel."
            }
            Some(Callback::Error { .. }) => {
                "The sign-in was not completed. You can close this window; MCPanel shows the details."
            }
            None => "This address is only used by MCPanel's sign-in.",
        };
        let _ = respond(&mut stream, if cb.is_some() { 200 } else { 400 }, page).await;
        cb
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hex = |b: u8| (b as char).to_digit(16).unwrap_or(0) as u8;
                out.push(hex(bytes[i + 1]) * 16 + hex(bytes[i + 2]));
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn parse_query(query: &str) -> Option<Callback> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error" => error = Some(v),
            "error_description" => description = Some(v),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Some(Callback::Error {
            error,
            description,
            state,
        });
    }
    Some(Callback::Code {
        code: code.filter(|c| !c.is_empty())?,
        state: state?,
    })
}

async fn respond(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>MCPanel</title></head>\
         <body style=\"font-family:system-ui,sans-serif;margin:3rem\"><h1>MCPanel</h1><p>{message}</p></body></html>"
    );
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[test]
    fn queries_are_parsed_and_decoded() {
        assert_eq!(
            parse_query("code=a%2Fb+c&state=xyz&scope=x"),
            Some(Callback::Code {
                code: "a/b c".into(),
                state: "xyz".into()
            })
        );
        assert_eq!(
            parse_query("error=access_denied&error_description=The+user+said+no&state=s"),
            Some(Callback::Error {
                error: "access_denied".into(),
                description: Some("The user said no".into()),
                state: Some("s".into())
            })
        );
        assert_eq!(parse_query("state=s"), None);
        assert_eq!(parse_query("code=&state=s"), None);
        let _ = decode("%");
        let _ = decode("%4");
        assert_eq!(decode("%41%zz"), "A%zz");
    }

    #[tokio::test]
    async fn receives_the_redirect_and_ignores_other_requests() {
        let lb = Loopback::bind("127.0.0.1", 0, "/").await.unwrap();
        let port = lb.port;
        let waiter = tokio::spawn(lb.wait(Duration::from_secs(10)));
        let lb2 = Loopback::bind("127.0.0.1", 0, "/oauth/callback")
            .await
            .unwrap();
        let p2 = lb2.port;
        let w2 = tokio::spawn(lb2.wait(Duration::from_secs(10)));
        let r = get(p2, "/favicon.ico").await;
        assert!(r.starts_with("HTTP/1.1 404"));
        let r = get(p2, "/oauth/callback?code=c1&state=s1").await;
        assert!(r.starts_with("HTTP/1.1 200") && r.contains("connected"));
        assert_eq!(
            w2.await.unwrap().unwrap(),
            Callback::Code {
                code: "c1".into(),
                state: "s1".into()
            }
        );
        let r = get(port, "/?error=access_denied&state=x").await;
        assert!(r.starts_with("HTTP/1.1 200"));
        assert!(matches!(
            waiter.await.unwrap().unwrap(),
            Callback::Error { .. }
        ));
    }

    #[tokio::test]
    async fn times_out_without_a_redirect() {
        let lb = Loopback::bind("localhost", 0, "/").await.unwrap();
        let e = lb.wait(Duration::from_millis(200)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Cancelled);
    }
}
