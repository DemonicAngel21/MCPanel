#![allow(clippy::unwrap_used)]
//! TPS/MSPT sampling end-to-end: real core and processes (`fake-mc` answering Paper's
//! `tps`/`mspt` and vanilla's `tick query`). The replies must not reach the console.

mod common;

use common::harness::*;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::software::TpsSource;
use std::time::{Duration, Instant};

async fn first_sample(
    h: &Harness,
    id: mcpanel_core::ids::ServerId,
) -> mcpanel_core::perf::TickSample {
    let start = Instant::now();
    loop {
        let m = h.core.monitor.server(id, None);
        if let Some(t) = m.tick {
            return t;
        }
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "no TPS/MSPT sample"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn console_has(h: &Harness, id: mcpanel_core::ids::ServerId, needle: &str) -> bool {
    h.core
        .servers
        .console(id)
        .snapshot(None, 500)
        .iter()
        .any(|l| l.text.contains(needle))
}

#[tokio::test(flavor = "multi_thread")]
async fn paper_and_vanilla_sources_are_sampled_silently() {
    let h = harness().await;
    let (paper, _) = add_server_as(&h, "p", "fake-paper", "{}", true, 60).await;
    let (vanilla, _) = add_server_as(&h, "v", "fake-vanilla", "{}", true, 60).await;
    for id in [paper, vanilla] {
        h.core.servers.start(id, "test").await.unwrap();
        wait_state(&h, id, LifecycleState::Running, T).await;
    }

    let p = first_sample(&h, paper).await;
    assert_eq!(p.source, TpsSource::PaperCommands);
    assert_eq!((p.tps, p.tps_calculated), (Some(20.0), false));
    assert_eq!((p.mspt, p.mspt_high), (Some(2.5), Some(9.5)));

    let v = first_sample(&h, vanilla).await;
    assert_eq!(v.source, TpsSource::VanillaTickQuery);
    assert_eq!((v.tps, v.tps_calculated), (Some(20.0), true));
    assert_eq!((v.mspt, v.mspt_high), (Some(2.5), Some(4.0)));
    assert_eq!(
        h.core.monitor.server(vanilla, None).tick_source,
        Some(TpsSource::VanillaTickQuery)
    );

    // Neither the queries nor their replies are shown in the console.
    for (id, needles) in [
        (paper, &["TPS from last", "Server tick times"][..]),
        (
            vanilla,
            &["Target tick rate", "Average time per tick", "tick query"][..],
        ),
    ] {
        for n in needles {
            assert!(!console_has(&h, id, n), "console shows {n}");
        }
    }
    let echoed = h
        .core
        .servers
        .console(paper)
        .snapshot(None, 500)
        .into_iter()
        .any(|l| l.stream == mcpanel_core::console::ConsoleStream::Command);
    assert!(!echoed, "probe commands are not echoed");
    // A user's own command still shows its reply once the probe is not waiting.
    h.core.servers.send_command(paper, "say hi").await.unwrap();
    assert!(wait_until(T, || console_has(&h, paper, "[Server] hi")).await);

    // The plain fake software has no source.
    let (plain, _) = add_server(&h, "x", "{}", true, 60).await;
    assert_eq!(h.core.monitor.server(plain, None).tick_source, None);

    for id in [paper, vanilla] {
        h.core.servers.stop(id, false, "test").await.unwrap();
        wait_state(&h, id, LifecycleState::Stopped, T).await;
    }
    h.finish().await;
}

/// A server that does not understand the queries is asked at most three times per
/// session (each unanswered query leaves an error line in its console). Takes about a
/// minute: the sampler runs every 15 seconds.
#[tokio::test(flavor = "multi_thread")]
async fn unanswered_queries_stop_after_three_attempts() {
    let h = harness().await;
    let (id, _) = add_server_as(
        &h,
        "old",
        "fake-paper",
        r#"{"no_perf_commands": true}"#,
        true,
        60,
    )
    .await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;
    let unknown = || {
        h.core
            .servers
            .console(id)
            .snapshot(None, 500)
            .iter()
            .filter(|l| l.text.contains("Unknown command: tps"))
            .count()
    };
    // Three misses happen within ~45 s; wait for two more intervals after that.
    assert!(wait_until(Duration::from_secs(60), || unknown() >= 3).await);
    tokio::time::sleep(Duration::from_secs(32)).await;
    assert_eq!(unknown(), 3, "queried again after three misses");
    assert!(h.core.monitor.server(id, None).tick.is_none());
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    h.finish().await;
}
