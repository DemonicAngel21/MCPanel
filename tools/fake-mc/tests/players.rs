#![allow(clippy::unwrap_used)]
//! Player management end-to-end tests: real core, SQLite and files, with `fake-mc` as
//! the running server (26.x-style "System chat:" output).

mod common;

use common::harness::*;
use mcpanel_core::error::ErrorCode;
use mcpanel_core::ids::ServerId;
use mcpanel_core::lifecycle::LifecycleState;
use mcpanel_core::players::{AppliedVia, PlayerAction};
use std::path::PathBuf;
use std::time::Duration;

fn set_props(h: &Harness, name: &str, extra: &str) -> PathBuf {
    let dir = h.servers.path().join(name);
    let props = std::fs::read_to_string(dir.join("server.properties")).unwrap();
    std::fs::write(dir.join("server.properties"), format!("{props}{extra}")).unwrap();
    dir
}

fn json(dir: &std::path::Path, file: &str) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join(file)).unwrap()).unwrap()
}

async fn act(h: &Harness, id: ServerId, a: PlayerAction) -> mcpanel_core::players::ActionOutcome {
    h.core.players.apply(id, a, "test").await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn stopped_server_lists_are_edited_in_place() {
    let h = harness().await;
    let (id, _) = add_server(&h, "files", "{}", true, 60).await;
    let dir = set_props(&h, "files", "online-mode=false\nop-permission-level=3\n");
    // An existing entry with a field MCPanel does not know.
    std::fs::write(
        dir.join("ops.json"),
        r#"[{"uuid":"00000000-0000-3000-8000-000000000001","name":"Old","level":2,"bypassesPlayerLimit":true,"note":"keep"}]"#,
    )
    .unwrap();

    let o = act(
        &h,
        id,
        PlayerAction::WhitelistAdd {
            name: "Alex".into(),
        },
    )
    .await;
    assert_eq!(o.via, AppliedVia::Files);
    let w = json(&dir, "whitelist.json");
    assert_eq!(w[0]["name"], "Alex");
    assert_eq!(
        w[0]["uuid"],
        mcpanel_core::players::offline_uuid("Alex").to_string(),
        "offline-mode servers use the offline UUID"
    );
    // Adding twice changes nothing.
    act(
        &h,
        id,
        PlayerAction::WhitelistAdd {
            name: "alex".into(),
        },
    )
    .await;
    assert_eq!(json(&dir, "whitelist.json").as_array().unwrap().len(), 1);

    act(
        &h,
        id,
        PlayerAction::Op {
            name: "Alex".into(),
        },
    )
    .await;
    let ops = json(&dir, "ops.json");
    assert_eq!(ops[0]["note"], "keep", "unknown fields survive");
    assert_eq!(ops[1]["name"], "Alex");
    assert_eq!(ops[1]["level"], 3, "op-permission-level is used");

    act(
        &h,
        id,
        PlayerAction::Ban {
            name: "Griefer".into(),
            reason: Some("Griefing".into()),
        },
    )
    .await;
    let bans = json(&dir, "banned-players.json");
    assert_eq!(bans[0]["reason"], "Griefing");
    assert_eq!(bans[0]["expires"], "forever");
    assert_eq!(bans[0]["source"], "MCPanel");
    act(
        &h,
        id,
        PlayerAction::BanIp {
            ip: "10.1.2.3".into(),
            reason: None,
        },
    )
    .await;
    assert_eq!(json(&dir, "banned-ips.json")[0]["ip"], "10.1.2.3");

    act(&h, id, PlayerAction::SetWhitelist { enabled: true }).await;
    let view = h.core.players.view(id).await.unwrap();
    assert!(view.whitelist_enabled && !view.online_mode && !view.live);
    assert_eq!(view.operators.len(), 2);
    assert_eq!(view.bans[0].target, "Griefer");
    assert_eq!(view.ip_bans[0].target, "10.1.2.3");
    let alex = view.known.iter().find(|k| k.name == "Alex").unwrap();
    assert!(alex.whitelisted && alex.op_level == Some(3));

    act(
        &h,
        id,
        PlayerAction::Pardon {
            name: "griefer".into(),
        },
    )
    .await;
    act(
        &h,
        id,
        PlayerAction::PardonIp {
            ip: "10.1.2.3".into(),
        },
    )
    .await;
    act(
        &h,
        id,
        PlayerAction::Deop {
            name: "ALEX".into(),
        },
    )
    .await;
    act(
        &h,
        id,
        PlayerAction::WhitelistRemove {
            name: "Alex".into(),
        },
    )
    .await;
    assert_eq!(json(&dir, "banned-players.json"), serde_json::json!([]));
    assert_eq!(json(&dir, "ops.json").as_array().unwrap().len(), 1);
    assert_eq!(json(&dir, "whitelist.json"), serde_json::json!([]));

    // Kicking needs a running server; invalid input never reaches the files.
    let err = h
        .core
        .players
        .apply(
            id,
            PlayerAction::Kick {
                name: "Alex".into(),
                reason: None,
            },
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ServerNotRunning);
    for bad in [
        PlayerAction::Op {
            name: "bad name".into(),
        },
        PlayerAction::BanIp {
            ip: "not-an-ip".into(),
            reason: None,
        },
    ] {
        assert_eq!(
            h.core
                .players
                .apply(id, bad, "test")
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn online_mode_names_are_resolved_and_corrupt_lists_are_left_alone() {
    let h = harness().await;
    let (id, _) = add_server(&h, "online", "{}", true, 60).await;
    let dir = h.servers.path().join("online");
    act(
        &h,
        id,
        PlayerAction::WhitelistAdd {
            name: "notch".into(),
        },
    )
    .await;
    let w = json(&dir, "whitelist.json");
    assert_eq!(w[0]["uuid"], NOTCH_UUID);
    assert_eq!(w[0]["name"], "Notch", "the account's exact name is used");
    let err = h
        .core
        .players
        .apply(
            id,
            PlayerAction::Op {
                name: "NoSuchPlayer".into(),
            },
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    std::fs::write(dir.join("ops.json"), "{broken").unwrap();
    let err = h
        .core
        .players
        .apply(
            id,
            PlayerAction::Op {
                name: "Notch".into(),
            },
            "test",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert_eq!(
        std::fs::read_to_string(dir.join("ops.json")).unwrap(),
        "{broken"
    );
    h.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn running_server_gets_commands_and_sessions_are_recorded() {
    let h = harness().await;
    let (id, _) = add_server(&h, "live", r#"{"system_chat": true}"#, true, 60).await;
    h.core.servers.start(id, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Running, T).await;

    h.core.servers.send_command(id, "join Alex").await.unwrap();
    let players = std::sync::Arc::clone(&h.core.players);
    assert!(
        wait_until(T, || tokio::task::block_in_place(
            || tokio::runtime::Handle::current()
                .block_on(players.view(id))
                .unwrap()
                .online
                == vec!["Alex".to_string()]
        ))
        .await,
        "26.x-style join tracked"
    );

    let o = act(
        &h,
        id,
        PlayerAction::Op {
            name: "Alex".into(),
        },
    )
    .await;
    assert_eq!(o.via, AppliedVia::Console);
    assert_eq!(
        o.messages,
        vec!["Made Alex a server operator"],
        "reply captured without the prefix"
    );

    tokio::time::sleep(Duration::from_millis(1100)).await;
    let o = act(
        &h,
        id,
        PlayerAction::Kick {
            name: "Alex".into(),
            reason: Some("Bye now".into()),
        },
    )
    .await;
    assert_eq!(o.messages[0], "Kicked Alex: Bye now");
    let view = h.core.players.view(id).await.unwrap();
    assert!(view.live && view.online.is_empty());

    // Stopping ends open sessions; play time comes from completed sessions.
    h.core.servers.send_command(id, "join Steve").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.core.servers.stop(id, false, "test").await.unwrap();
    wait_state(&h, id, LifecycleState::Stopped, T).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let history = h.core.players.history(id).await.unwrap();
    let alex = history.iter().find(|s| s.name == "Alex").unwrap();
    assert_eq!(alex.sessions, 1);
    assert!(alex.total_play_ms >= 1000, "{alex:?}");
    let steve = history.iter().find(|s| s.name == "Steve").unwrap();
    assert!(
        steve.total_play_ms > 0,
        "session closed when the server stopped"
    );
    let known = h.core.players.view(id).await.unwrap().known;
    assert!(
        known
            .iter()
            .any(|k| k.name == "Steve" && k.stats.is_some() && !k.online)
    );

    // Actions are audited with the player name only.
    let audit = h
        .core
        .audit
        .query(&mcpanel_core::model::AuditQuery {
            server_id: Some(id),
            before: None,
            limit: 50,
        })
        .await
        .unwrap();
    assert!(
        audit
            .iter()
            .any(|a| a.action == "player.op" && a.target.as_deref() == Some("Alex"))
    );
    h.finish().await;
}
