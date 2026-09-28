#![allow(clippy::unwrap_used)]
//! Real-server player tests: a headless offline-mode client (`common::mc_client`) joins
//! a LOCAL test server, so join/leave detection, console commands and file edits are
//! checked against real Minecraft output. Requires the Minecraft EULA acceptance flag:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_players -- --nocapture
//!
//! Online-mode (authenticated) logins cannot be automated without a Minecraft account;
//! they are not covered here.

mod common;

use common::mc_client;
use common::real::{KillOnPanic, RealServer, eula_accepted};
use mcpanel_core::players::{AppliedVia, PlayerAction};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PORT: u16 = 25612;

async fn join(name: &'static str) -> std::io::Result<mc_client::Player> {
    tokio::task::spawn_blocking(move || mc_client::join(PORT, name))
        .await
        .unwrap()
}

async fn wait_online(s: &RealServer, want: &[&str]) {
    let start = Instant::now();
    loop {
        let online = s.core.players.view(s.id).await.unwrap().online;
        if online.iter().map(String::as_str).collect::<Vec<_>>() == want {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "online players {online:?}, expected {want:?}\n{}",
            s.console_tail(40)
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn act(s: &RealServer, a: PlayerAction) -> Vec<String> {
    let o = s.core.players.apply(s.id, a, "e2e").await.unwrap();
    eprintln!("{:?}: {:?}", o.via, o.messages);
    o.messages
}

#[tokio::test(flavor = "multi_thread")]
async fn players_join_leave_commands_and_file_edits() {
    if !eula_accepted() {
        return;
    }
    let s = RealServer::create(&[
        ("server-port", "25612"),
        ("online-mode", "false"),
        ("network-compression-threshold", "-1"),
    ])
    .await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    s.start_and_wait().await;
    let view = s.core.players.view(s.id).await.unwrap();
    eprintln!(
        "{} {}: generated white-list={}",
        s.software, s.version, view.whitelist_enabled
    );
    assert!(view.live && !view.online_mode);

    // Turning the whitelist off goes through the console.
    let reply = act(&s, PlayerAction::SetWhitelist { enabled: false }).await;
    assert!(
        reply
            .iter()
            .any(|m| m.contains("Whitelist is now turned off")),
        "{reply:?}"
    );

    // Join is detected from the real log line.
    let p = join("McpTester").await.unwrap();
    wait_online(&s, &["McpTester"]).await;

    let reply = act(
        &s,
        PlayerAction::Op {
            name: "McpTester".into(),
        },
    )
    .await;
    assert_eq!(reply, vec!["Made McpTester a server operator"]);
    let reply = act(
        &s,
        PlayerAction::Op {
            name: "McpTester".into(),
        },
    )
    .await;
    assert!(reply[0].starts_with("Nothing changed"), "{reply:?}");
    act(
        &s,
        PlayerAction::WhitelistAdd {
            name: "McpTester".into(),
        },
    )
    .await;
    let view = s.core.players.view(s.id).await.unwrap();
    let me = view.known.iter().find(|k| k.name == "McpTester").unwrap();
    assert!(
        me.online && me.op_level == Some(4) && me.whitelisted,
        "{me:?}"
    );
    assert_eq!(
        me.uuid,
        Some(mcpanel_core::players::offline_uuid("McpTester"))
    );

    // Kick disconnects the client and is detected as a leave.
    let reply = act(
        &s,
        PlayerAction::Kick {
            name: "McpTester".into(),
            reason: Some("Bye now".into()),
        },
    )
    .await;
    assert!(reply[0].contains("Kicked McpTester"), "{reply:?}");
    wait_online(&s, &[]).await;
    assert!(p.leave().unwrap().contains("Bye now"));

    // Ban blocks the login; pardon lets the player back in; leaving is detected.
    act(
        &s,
        PlayerAction::Ban {
            name: "McpTester".into(),
            reason: Some("Testing bans".into()),
        },
    )
    .await;
    let err = join("McpTester")
        .await
        .err()
        .expect("banned player cannot join");
    eprintln!("banned login: {err}");
    act(
        &s,
        PlayerAction::Pardon {
            name: "McpTester".into(),
        },
    )
    .await;
    let p = join("McpTester").await.unwrap();
    wait_online(&s, &["McpTester"]).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    p.leave().unwrap();
    wait_online(&s, &[]).await;

    // While stopped, MCPanel edits the files; the real server honours them on start.
    s.stop_and_wait().await;
    let reply = act(
        &s,
        PlayerAction::Deop {
            name: "McpTester".into(),
        },
    )
    .await;
    assert_eq!(reply, vec!["McpTester is no longer an operator"]);
    let o = s
        .core
        .players
        .apply(
            s.id,
            PlayerAction::WhitelistAdd {
                name: "OfflineAdd".into(),
            },
            "e2e",
        )
        .await
        .unwrap();
    assert_eq!(o.via, AppliedVia::Files);
    act(&s, PlayerAction::SetWhitelist { enabled: true }).await;
    s.start_and_wait().await;
    let reply = act(
        &s,
        PlayerAction::Op {
            name: "McpTester".into(),
        },
    )
    .await;
    assert_eq!(
        reply,
        vec!["Made McpTester a server operator"],
        "the deop written to ops.json was loaded"
    );
    // The whitelist edited on disk is in force: McpTester (listed earlier) may join,
    // a stranger may not.
    let p = join("McpTester").await.unwrap();
    wait_online(&s, &["McpTester"]).await;
    assert!(join("Stranger").await.is_err(), "whitelist enforced");
    drop(p);
    wait_online(&s, &[]).await;

    let history = s.core.players.history(s.id).await.unwrap();
    let me = history.iter().find(|h| h.name == "McpTester").unwrap();
    eprintln!("history: {me:?}");
    assert_eq!(me.sessions, 3);
    assert!(me.total_play_ms > 1000);

    s.stop_and_wait().await;
    s.finish().await;
}
