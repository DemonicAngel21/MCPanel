//! Minecraft-level performance (TPS/MSPT) from the server's own commands, sampled while
//! a server runs. Verified 2026-09-29 against real servers:
//!
//! - Vanilla `/tick query` (1.20.3+; also on Paper and mod loaders): "The game is running
//!   normally" (or "…can't keep up…", frozen, sprinting), "Target tick rate: 20.0 per
//!   second.", an unprefixed "Average time per tick: 0.3ms (Target: 50.0ms)" and
//!   "Percentiles: P50: 0.3ms P95: 0.4ms P99: 1.4ms. Sample: 100" (1.21.4: ", sample:").
//!   26.x prefixes the lines with "System chat: ". Vanilla reports no TPS: MCPanel shows
//!   `min(target, 1000 / average MSPT)` and marks it as calculated.
//! - Paper/Purpur `tps`: "TPS from last 1m, 5m, 15m: 20.0, 20.0, 20.0" (values above 20
//!   carry a leading `*`); `mspt`: "Server tick times (avg/min/max) from last 5s, 10s,
//!   1m:" followed by "◴ 0.4/0.3/4.8, 1.0/0.3/35.9, 1.0/0.3/35.9".
//!
//! The replies are diverted from the console view (they still reach the server's own
//! log file).

use crate::console::dialect;
use crate::software::TpsSource;
use crate::time::Timestamp;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// `/tick query` exists from this Minecraft version on.
pub const TICK_QUERY_SINCE: &str = "1.20.3";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickSample {
    pub at: Timestamp,
    pub source: TpsSource,
    pub tps: Option<f32>,
    /// `tps` was calculated from MSPT (vanilla reports no TPS).
    pub tps_calculated: bool,
    /// Average milliseconds per tick (vanilla: recent samples; Paper: last 5 s).
    pub mspt: Option<f32>,
    /// Vanilla P95 or Paper's 5 s maximum.
    pub mspt_high: Option<f32>,
    /// Vanilla status line, e.g. "The game is running normally".
    pub status: Option<String>,
}

/// `version >= since` for plain release numbers ("1.21.4", "26.3"); `None` for
/// anything else (snapshots, pre-releases). Used when the version catalog is
/// unavailable (offline).
pub fn release_at_least(version: &str, since: &str) -> Option<bool> {
    let parse = |v: &str| -> Option<Vec<u32>> {
        v.split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<Vec<u32>>>()
    };
    let (mut a, mut b) = (parse(version)?, parse(since)?);
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    Some(a >= b)
}

/// The message of a console line without log prefix and 26.x "System chat: ".
pub fn message(raw: &str) -> String {
    let m = dialect::parse_line(raw).message;
    m.strip_prefix("System chat: ").unwrap_or(&m).to_string()
}

static TARGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Target tick rate: ([\d.]+) per second").expect("regex"));
static AVERAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Average time per tick: ([\d.]+) ?ms").expect("regex"));
static PERCENTILES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Percentiles: P50: ([\d.]+) ?ms P95: ([\d.]+) ?ms P99: ([\d.]+) ?ms")
        .expect("regex")
});
static PAPER_TPS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^TPS from last 1m, 5m, 15m: \*?([\d.]+), \*?([\d.]+), \*?([\d.]+)").expect("regex")
});
static PAPER_MSPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\S?\s*([\d.]+)/([\d.]+)/([\d.]+), [\d.]+/[\d.]+/[\d.]+, [\d.]+/[\d.]+/[\d.]+$")
        .expect("regex")
});

/// The command(s) to send for a source.
pub fn commands(source: TpsSource) -> &'static [&'static str] {
    match source {
        TpsSource::VanillaTickQuery => &["tick query"],
        TpsSource::PaperCommands => &["tps", "mspt"],
    }
}

/// Whether a reply line belongs to the query (so it is diverted from the console).
pub fn is_reply(source: TpsSource, msg: &str) -> bool {
    match source {
        TpsSource::VanillaTickQuery => {
            msg.starts_with("The game is ")
                || TARGET.is_match(msg)
                || AVERAGE.is_match(msg)
                || PERCENTILES.is_match(msg)
        }
        TpsSource::PaperCommands => {
            PAPER_TPS.is_match(msg)
                || msg.starts_with("Server tick times (avg/min/max) from last")
                || PAPER_MSPT.is_match(msg)
        }
    }
}

/// Whether the collected reply is complete.
pub fn is_complete(source: TpsSource, msgs: &[String]) -> bool {
    match source {
        TpsSource::VanillaTickQuery => msgs.iter().any(|m| PERCENTILES.is_match(m)),
        TpsSource::PaperCommands => {
            msgs.iter().any(|m| PAPER_TPS.is_match(m))
                && msgs.iter().any(|m| PAPER_MSPT.is_match(m))
        }
    }
}

fn num(c: &regex::Captures<'_>, i: usize) -> Option<f32> {
    c.get(i)?
        .as_str()
        .parse()
        .ok()
        .filter(|v: &f32| v.is_finite())
}

/// Turn the reply messages into a sample (`None` if nothing was recognised).
pub fn parse(source: TpsSource, msgs: &[String], at: Timestamp) -> Option<TickSample> {
    let find = |re: &Regex| msgs.iter().find_map(|m| re.captures(m));
    match source {
        TpsSource::VanillaTickQuery => {
            let mspt = find(&AVERAGE).and_then(|c| num(&c, 1));
            let target = find(&TARGET).and_then(|c| num(&c, 1));
            let p95 = find(&PERCENTILES).and_then(|c| num(&c, 2));
            let status = msgs
                .iter()
                .find(|m| m.starts_with("The game is "))
                .map(|s| s.trim_end_matches('.').to_string());
            if mspt.is_none() && status.is_none() {
                return None;
            }
            let frozen = status.as_deref().is_some_and(|s| s.contains("frozen"));
            let sprinting = status.as_deref().is_some_and(|s| s.contains("sprinting"));
            let tps = match (mspt, target) {
                _ if frozen => None,
                (Some(ms), _) if sprinting && ms > 0.0 => Some(1000.0 / ms),
                (Some(ms), Some(t)) if ms > 0.0 => Some(t.min(1000.0 / ms)),
                (Some(_), Some(t)) => Some(t),
                _ => None,
            };
            Some(TickSample {
                at,
                source,
                tps,
                tps_calculated: tps.is_some(),
                mspt,
                mspt_high: p95,
                status,
            })
        }
        TpsSource::PaperCommands => {
            let tps = find(&PAPER_TPS).and_then(|c| num(&c, 1));
            let m = find(&PAPER_MSPT);
            let mspt = m.as_ref().and_then(|c| num(c, 1));
            let high = m.as_ref().and_then(|c| num(c, 3));
            if tps.is_none() && mspt.is_none() {
                return None;
            }
            Some(TickSample {
                at,
                source,
                tps,
                tps_calculated: false,
                mspt,
                mspt_high: high,
                status: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| message(l)).collect()
    }

    #[test]
    fn vanilla_26_tick_query() {
        let m = msgs(&[
            "[15:12:47] [Server thread/INFO]: System chat: The game is running normally",
            "[15:12:47] [Server thread/INFO]: System chat: Target tick rate: 20.0 per second.",
            "Average time per tick: 0.3ms (Target: 50.0ms)",
            "[15:12:47] [Server thread/INFO]: System chat: Percentiles: P50: 0.3ms P95: 0.4ms P99: 1.4ms. Sample: 100",
        ]);
        for x in &m {
            assert!(is_reply(TpsSource::VanillaTickQuery, x), "{x}");
        }
        assert!(is_complete(TpsSource::VanillaTickQuery, &m));
        let s = parse(TpsSource::VanillaTickQuery, &m, Timestamp(1)).unwrap();
        assert_eq!(s.tps, Some(20.0));
        assert!(s.tps_calculated);
        assert_eq!(s.mspt, Some(0.3));
        assert_eq!(s.mspt_high, Some(0.4));
        assert_eq!(s.status.as_deref(), Some("The game is running normally"));
    }

    #[test]
    fn vanilla_1_21_lagging() {
        let m = msgs(&[
            "[15:15:54] [Server thread/INFO]: The game is running, but can't keep up with the target tick rate",
            "[15:15:54] [Server thread/INFO]: Target tick rate: 20.0 per second.",
            "Average time per tick: 80.0ms (Target: 50.0ms)",
            "[15:15:54] [Server thread/INFO]: Percentiles: P50: 70.4ms P95: 91.9ms P99: 112.9ms, sample: 100",
        ]);
        let s = parse(TpsSource::VanillaTickQuery, &m, Timestamp(1)).unwrap();
        assert_eq!(s.tps, Some(12.5));
        assert_eq!(s.mspt_high, Some(91.9));
    }

    #[test]
    fn frozen_game_has_no_tps() {
        let m = msgs(&[
            "[1] [Server thread/INFO]: The game is frozen",
            "[1] [Server thread/INFO]: Target tick rate: 20.0 per second.",
            "Average time per tick: 0.1ms (Target: 50.0ms)",
        ]);
        let s = parse(TpsSource::VanillaTickQuery, &m, Timestamp(1)).unwrap();
        assert_eq!(s.tps, None);
        assert!(!s.tps_calculated);
    }

    #[test]
    fn paper_tps_and_mspt() {
        let m = msgs(&[
            "[15:14:22 INFO]: TPS from last 1m, 5m, 15m: *20.0, 19.5, 18.25",
            "[15:14:24 INFO]: Server tick times (avg/min/max) from last 5s, 10s, 1m:",
            "[15:14:24 INFO]: ◴ 0.4/0.3/4.8, 1.0/0.3/35.9, 1.0/0.3/35.9",
        ]);
        for x in &m {
            assert!(is_reply(TpsSource::PaperCommands, x), "{x}");
        }
        assert!(is_complete(TpsSource::PaperCommands, &m));
        let s = parse(TpsSource::PaperCommands, &m, Timestamp(1)).unwrap();
        assert_eq!(s.tps, Some(20.0));
        assert!(!s.tps_calculated);
        assert_eq!((s.mspt, s.mspt_high), (Some(0.4), Some(4.8)));
    }

    #[test]
    fn release_comparison_fallback() {
        assert_eq!(release_at_least("1.21.4", "1.20.3"), Some(true));
        assert_eq!(release_at_least("1.20.3", "1.20.3"), Some(true));
        assert_eq!(release_at_least("1.20.2", "1.20.3"), Some(false));
        assert_eq!(release_at_least("1.20", "1.20.3"), Some(false));
        assert_eq!(release_at_least("26.3", "1.20.3"), Some(true));
        assert_eq!(release_at_least("26.3-snapshot-2", "1.20.3"), None);
    }

    #[test]
    fn unrelated_lines_are_not_replies() {
        for l in [
            "Steve joined the game",
            "Done (1.2s)! For help, type \"help\"",
            "TPS is great",
        ] {
            assert!(!is_reply(TpsSource::VanillaTickQuery, l));
            assert!(!is_reply(TpsSource::PaperCommands, l));
        }
    }
}
