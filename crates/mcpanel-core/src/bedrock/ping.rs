//! RakNet "unconnected ping" — how Bedrock clients list a server. Verified 2026-09-29
//! against Geyser 2.11.3: ping = `0x01, time: i64, MAGIC, client guid: i64`; pong =
//! `0x1c, time: i64, server guid: i64, MAGIC, len: u16, "MCPE;motd;protocol;version;
//! players;max;guid;sub-motd;gamemode;gamemode-id;port-v4;port-v6;"` (big endian).

use crate::error::{CoreError, CoreResult, ErrorCode};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

const MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];
const UNCONNECTED_PING: u8 = 0x01;
const UNCONNECTED_PONG: u8 = 0x1c;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BedrockPong {
    pub edition: String,
    pub motd: String,
    pub protocol: Option<u32>,
    pub version: String,
    pub players: Option<u32>,
    pub max_players: Option<u32>,
    pub sub_motd: Option<String>,
    pub game_mode: Option<String>,
    pub latency_ms: u64,
}

pub fn encode_ping(time_ms: i64, client_guid: i64) -> Vec<u8> {
    let mut p = Vec::with_capacity(33);
    p.push(UNCONNECTED_PING);
    p.extend_from_slice(&time_ms.to_be_bytes());
    p.extend_from_slice(&MAGIC);
    p.extend_from_slice(&client_guid.to_be_bytes());
    p
}

pub fn decode_pong(d: &[u8], latency_ms: u64) -> CoreResult<BedrockPong> {
    let bad = || CoreError::new(ErrorCode::ProviderError, "Not a valid Bedrock ping reply");
    if d.len() < 35 || d[0] != UNCONNECTED_PONG || d[17..33] != MAGIC {
        return Err(bad());
    }
    let len = u16::from_be_bytes([d[33], d[34]]) as usize;
    let s = d.get(35..35 + len).ok_or_else(bad)?;
    let s = String::from_utf8_lossy(s);
    let f: Vec<&str> = s.split(';').collect();
    if f.len() < 6 {
        return Err(bad());
    }
    let opt = |i: usize| f.get(i).filter(|v| !v.is_empty()).map(|v| v.to_string());
    Ok(BedrockPong {
        edition: f[0].to_string(),
        motd: f[1].to_string(),
        protocol: f[2].parse().ok(),
        version: f[3].to_string(),
        players: f[4].parse().ok(),
        max_players: f[5].parse().ok(),
        sub_motd: opt(7),
        game_mode: opt(8),
        latency_ms,
    })
}

/// Ping a Bedrock listener (3 tries, `timeout` each).
pub async fn ping(addr: SocketAddr, timeout: Duration) -> CoreResult<BedrockPong> {
    let bind: SocketAddr = if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    }
    .parse()
    .map_err(|_| CoreError::internal("bad bind address"))?;
    let sock = tokio::net::UdpSocket::bind(bind)
        .await
        .map_err(|e| CoreError::io("Cannot open a UDP socket", &e))?;
    sock.connect(addr)
        .await
        .map_err(|e| CoreError::io("Cannot reach the Bedrock port", &e))?;
    let guid = i64::from_be_bytes(
        uuid::Uuid::new_v4().as_bytes()[..8]
            .try_into()
            .unwrap_or([0; 8]),
    );
    let mut buf = [0u8; 2048];
    for _ in 0..3 {
        let start = std::time::Instant::now();
        let now = crate::time::Timestamp::now().0;
        // An ICMP "port unreachable" surfaces as a send/recv error; retry until the end.
        if sock.send(&encode_ping(now, guid)).await.is_err() {
            tokio::time::sleep(timeout).await;
            continue;
        }
        if let Ok(Ok(n)) = tokio::time::timeout(timeout, sock.recv(&mut buf)).await {
            return decode_pong(&buf[..n], start.elapsed().as_millis() as u64);
        }
    }
    Err(CoreError::new(
        ErrorCode::ProviderUnavailable,
        format!("No Bedrock reply from UDP port {}", addr.port()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pong(text: &str) -> Vec<u8> {
        let mut d = vec![UNCONNECTED_PONG];
        d.extend_from_slice(&1i64.to_be_bytes());
        d.extend_from_slice(&7i64.to_be_bytes());
        d.extend_from_slice(&MAGIC);
        d.extend_from_slice(&(text.len() as u16).to_be_bytes());
        d.extend_from_slice(text.as_bytes());
        d
    }

    #[test]
    fn decodes_a_geyser_pong() {
        let p = decode_pong(
            &pong("MCPE;A Minecraft Server;2193;26.51;0;20;7934955399906986213;Another Geyser server.;Survival;1;19132;19132;"),
            3,
        )
        .unwrap();
        assert_eq!(p.motd, "A Minecraft Server");
        assert_eq!(p.protocol, Some(2193));
        assert_eq!(p.version, "26.51");
        assert_eq!((p.players, p.max_players), (Some(0), Some(20)));
        assert_eq!(p.sub_motd.as_deref(), Some("Another Geyser server."));
        assert_eq!(p.game_mode.as_deref(), Some("Survival"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_pong(b"\x1cshort", 0).is_err());
        let mut d = pong("MCPE;x;1;2;0;1;");
        d[18] = 0;
        assert!(decode_pong(&d, 0).is_err());
    }

    #[test]
    fn ping_packet_layout() {
        let p = encode_ping(5, 9);
        assert_eq!(p.len(), 33);
        assert_eq!(p[0], 1);
        assert_eq!(&p[9..25], &MAGIC);
    }

    #[tokio::test]
    async fn pings_a_local_responder() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            let (n, from) = server.recv_from(&mut buf).await.unwrap();
            assert_eq!(buf[0], UNCONNECTED_PING);
            assert_eq!(n, 33);
            server
                .send_to(&pong("MCPE;Hi;1;1.0;2;10;1;Sub;Creative;1;"), from)
                .await
                .unwrap();
        });
        let p = ping(addr, Duration::from_secs(2)).await.unwrap();
        assert_eq!(p.motd, "Hi");
        assert_eq!(p.players, Some(2));
    }
}
