#![allow(clippy::unwrap_used)]
//! Real TPS/MSPT sampling: vanilla 26.3 (`tick query`, "System chat:" prefixed) and
//! Paper 1.21.11 (`tps`/`mspt`) produce samples, and the replies stay out of the console.
//! Requires the Minecraft EULA acceptance flag and network access:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_performance -- --nocapture --test-threads 1

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted};
use mcpanel_core::software::TpsSource;
use std::sync::Arc;
use std::time::{Duration, Instant};

async fn sampled(software: &str, version: &str, port: &str, source: TpsSource) {
    let s = RealServer::create_with(software, Some(version), &[("server-port", port)]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    s.start_and_wait().await;
    let start = Instant::now();
    let sample = loop {
        if let Some(t) = s.core.monitor.server(s.id, None).tick {
            break t;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "no sample");
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    eprintln!("{software} {version}: {sample:?}");
    assert_eq!(sample.source, source);
    assert!(sample.mspt.is_some_and(|m| (0.0..1000.0).contains(&m)));
    assert!(sample.tps.is_some_and(|t| t > 0.0 && t <= 21.0));
    assert_eq!(sample.tps_calculated, source == TpsSource::VanillaTickQuery);
    let tail = s.console_tail(2000);
    for n in [
        "Target tick rate",
        "Average time per tick",
        "TPS from last",
        "Server tick times",
    ] {
        assert!(!tail.contains(n), "console shows {n}:\n{tail}");
    }
    s.stop_and_wait().await;
    s.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn vanilla_tick_query_is_sampled() {
    if !eula_accepted() {
        return;
    }
    sampled("vanilla", "26.3", "25643", TpsSource::VanillaTickQuery).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn paper_tps_and_mspt_are_sampled() {
    if !eula_accepted() {
        return;
    }
    sampled("paper", "1.21.11", "25644", TpsSource::PaperCommands).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fabric_uses_the_vanilla_tick_query() {
    if !eula_accepted() {
        return;
    }
    sampled("fabric", "26.2", "25645", TpsSource::VanillaTickQuery).await;
}
