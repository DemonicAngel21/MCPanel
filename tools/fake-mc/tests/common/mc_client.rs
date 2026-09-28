//! Minimal headless Minecraft Java Edition client for tests against a LOCAL server
//! running with `online-mode=false` and `network-compression-threshold=-1`.
//!
//! It performs a Server List Ping (to learn the protocol number), then logs in, answers
//! the configuration phase and keep-alives until told to leave — enough for the server
//! to log a player joining and leaving. Packet ids come from the protocol documentation
//! for each supported protocol (see `docs/architecture/verification-log.md`); an unknown
//! protocol is reported instead of guessed.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Packet ids that differ between protocol versions.
#[derive(Debug, Clone, Copy)]
pub struct PacketIds {
    // configuration state
    pub cfg_cb_disconnect: i32,
    pub cfg_cb_finish: i32,
    pub cfg_cb_keep_alive: i32,
    pub cfg_cb_ping: i32,
    pub cfg_cb_known_packs: i32,
    pub cfg_cb_code_of_conduct: Option<i32>,
    pub cfg_sb_finish_ack: i32,
    pub cfg_sb_keep_alive: i32,
    pub cfg_sb_pong: i32,
    pub cfg_sb_known_packs: i32,
    pub cfg_sb_accept_code_of_conduct: Option<i32>,
    // play state
    pub play_cb_keep_alive: i32,
    pub play_cb_disconnect: i32,
    pub play_sb_keep_alive: i32,
}

/// Protocol 777 = Minecraft 26.3 (minecraft.wiki, Java Edition protocol/Packets).
const P777: PacketIds = PacketIds {
    cfg_cb_disconnect: 0x02,
    cfg_cb_finish: 0x03,
    cfg_cb_keep_alive: 0x04,
    cfg_cb_ping: 0x05,
    cfg_cb_known_packs: 0x0F,
    cfg_cb_code_of_conduct: Some(0x14),
    cfg_sb_finish_ack: 0x03,
    cfg_sb_keep_alive: 0x04,
    cfg_sb_pong: 0x05,
    cfg_sb_known_packs: 0x07,
    cfg_sb_accept_code_of_conduct: Some(0x09),
    play_cb_keep_alive: 0x2D,
    play_cb_disconnect: 0x20,
    play_sb_keep_alive: 0x1C,
};

pub fn packet_ids(protocol: i32) -> Option<PacketIds> {
    match protocol {
        777 => Some(P777),
        _ => None,
    }
}

fn write_varint(out: &mut Vec<u8>, mut v: i32) {
    loop {
        let mut b = (v & 0x7F) as u8;
        v = ((v as u32) >> 7) as i32;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

fn read_varint(r: &mut impl Read) -> io::Result<i32> {
    let mut result = 0i32;
    for i in 0..5 {
        let mut b = [0u8];
        r.read_exact(&mut b)?;
        result |= ((b[0] & 0x7F) as i32) << (7 * i);
        if b[0] & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "varint too long",
    ))
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    write_varint(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

fn send(stream: &mut TcpStream, id: i32, body: &[u8]) -> io::Result<()> {
    let mut payload = Vec::new();
    write_varint(&mut payload, id);
    payload.extend_from_slice(body);
    let mut frame = Vec::new();
    write_varint(&mut frame, payload.len() as i32);
    frame.extend_from_slice(&payload);
    stream.write_all(&frame)
}

fn recv(stream: &mut TcpStream) -> io::Result<(i32, Vec<u8>)> {
    let len = read_varint(stream)?;
    if !(0..=8 * 1024 * 1024).contains(&len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad packet length",
        ));
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf)?;
    let mut cur = io::Cursor::new(buf);
    let id = read_varint(&mut cur)?;
    let pos = cur.position() as usize;
    Ok((id, cur.into_inner()[pos..].to_vec()))
}

fn handshake(stream: &mut TcpStream, protocol: i32, port: u16, intent: i32) -> io::Result<()> {
    let mut b = Vec::new();
    write_varint(&mut b, protocol);
    write_string(&mut b, "127.0.0.1");
    b.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut b, intent);
    send(stream, 0x00, &b)
}

/// Server List Ping (retried: a server that just became ready may drop the first
/// connection). Returns the status JSON.
pub fn status(port: u16) -> io::Result<serde_json::Value> {
    let mut last = None;
    for _ in 0..5 {
        match status_once(port) {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("status failed")))
}

fn status_once(port: u16) -> io::Result<serde_json::Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    handshake(&mut s, -1, port, 1)?;
    send(&mut s, 0x00, &[])?;
    let (id, body) = recv(&mut s)?;
    if id != 0x00 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected status packet",
        ));
    }
    let mut cur = io::Cursor::new(body);
    let len = read_varint(&mut cur)? as usize;
    let pos = cur.position() as usize;
    let json = &cur.into_inner()[pos..pos + len];
    serde_json::from_slice(json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// A connected player. Dropping it (or calling [`Player::leave`]) disconnects.
pub struct Player {
    stop: Arc<AtomicBool>,
    socket: TcpStream,
    thread: Option<std::thread::JoinHandle<io::Result<String>>>,
}

impl Player {
    /// Close the connection; returns why the session ended.
    pub fn leave(mut self) -> io::Result<String> {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
        self.thread.take().map_or(Ok(String::new()), |t| {
            t.join().unwrap_or_else(|_| Ok("panicked".into()))
        })
    }

    /// Whether the server has ended the session (kick, ban, stop).
    pub fn is_disconnected(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}

/// Log in as `name` and stay connected (answering keep-alives) until `leave`.
pub fn join(port: u16, name: &str) -> io::Result<Player> {
    let protocol = status(port)?["version"]["protocol"]
        .as_i64()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no protocol in status"))?
        as i32;
    let ids = packet_ids(protocol).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("protocol {protocol} is not supported by the test client"),
        )
    })?;
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(20)))?;
    handshake(&mut s, protocol, port, 2)?;
    let mut hello = Vec::new();
    write_string(&mut hello, name);
    hello.extend_from_slice(mcpanel_core::players::offline_uuid(name).as_bytes());
    send(&mut s, 0x00, &hello)?;

    // Login.
    loop {
        let (id, body) = recv(&mut s)?;
        match id {
            0x00 => {
                return Err(io::Error::other(format!(
                    "disconnected during login: {}",
                    String::from_utf8_lossy(&body)
                )));
            }
            0x01 => {
                return Err(io::Error::other(
                    "the server requires authentication (online-mode=true)",
                ));
            }
            0x02 => {
                send(&mut s, 0x03, &[])?; // Login Acknowledged
                break;
            }
            0x03 => {
                return Err(io::Error::other(
                    "compression is not supported; set network-compression-threshold=-1",
                ));
            }
            _ => {}
        }
    }

    // Configuration.
    loop {
        let (id, body) = recv(&mut s)?;
        if id == ids.cfg_cb_known_packs {
            let mut b = Vec::new();
            write_varint(&mut b, 0);
            send(&mut s, ids.cfg_sb_known_packs, &b)?;
        } else if id == ids.cfg_cb_keep_alive {
            send(&mut s, ids.cfg_sb_keep_alive, &body)?;
        } else if id == ids.cfg_cb_ping {
            send(&mut s, ids.cfg_sb_pong, &body)?;
        } else if Some(id) == ids.cfg_cb_code_of_conduct {
            if let Some(accept) = ids.cfg_sb_accept_code_of_conduct {
                send(&mut s, accept, &[])?;
            }
        } else if id == ids.cfg_cb_disconnect {
            return Err(io::Error::other(format!(
                "disconnected during configuration: {}",
                String::from_utf8_lossy(&body)
            )));
        } else if id == ids.cfg_cb_finish {
            send(&mut s, ids.cfg_sb_finish_ack, &[])?;
            break;
        }
    }

    // Play: answer keep-alives (blocking reads) until the socket is shut down.
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);
    s.set_read_timeout(None)?;
    let socket = s.try_clone()?;
    let thread = std::thread::spawn(move || -> io::Result<String> {
        loop {
            match recv(&mut s) {
                Ok((id, body)) if id == ids.play_cb_keep_alive => {
                    send(&mut s, ids.play_sb_keep_alive, &body)?
                }
                Ok((id, body)) if id == ids.play_cb_disconnect => {
                    return Ok(format!("disconnected: {}", String::from_utf8_lossy(&body)));
                }
                Ok(_) => {}
                Err(_) if stop2.load(Ordering::SeqCst) => return Ok("left".into()),
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Ok("connection closed by server".into());
                }
                Err(e) => return Err(e),
            }
        }
    });
    Ok(Player {
        stop,
        socket,
        thread: Some(thread),
    })
}
