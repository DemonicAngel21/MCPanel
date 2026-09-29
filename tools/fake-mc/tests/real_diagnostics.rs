#![allow(clippy::unwrap_used)]
//! Real crash analysis: a test plugin blocks Paper's server thread until Paper's
//! watchdog shuts the server down; MCPanel must record a watchdog crash that names the
//! plugin. The plugin is compiled with the `javac` next to the server's Java (the test
//! is skipped without a JDK). Requires the Minecraft EULA acceptance flag and network:
//!
//!   $env:MCPANEL_E2E_ACCEPT_MINECRAFT_EULA="yes"
//!   cargo test -p fake-mc --test real_diagnostics -- --nocapture

mod common;

use common::real::{KillOnPanic, RealServer, eula_accepted};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PLUGIN: &str = r#"package com.example.mcpaneltest;

import org.bukkit.plugin.java.JavaPlugin;

public final class CrashTest extends JavaPlugin {
    @Override
    public void onEnable() {
        getServer().getScheduler().runTaskLater(this, this::blockServerThread, 40L);
    }

    private void blockServerThread() {
        getLogger().info("Blocking the server thread");
        try {
            Thread.sleep(120_000L);
        } catch (InterruptedException ignored) {
        }
    }
}
"#;

fn jars_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            jars_under(&p, out);
        } else if p.extension().is_some_and(|x| x == "jar") {
            out.push(p);
        }
    }
}

/// Compile the test plugin against the server's own jars; `None` without a JDK.
fn build_plugin(server: &Path, java: &Path, work: &Path) -> Option<PathBuf> {
    let bin = java.parent()?;
    let (javac, jar) = (bin.join("javac.exe"), bin.join("jar.exe"));
    if !javac.is_file() || !jar.is_file() {
        return None;
    }
    let src = work.join("src/com/example/mcpaneltest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("CrashTest.java"), PLUGIN).unwrap();
    let out = work.join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("plugin.yml"),
        "name: CrashTest\nversion: '1.0'\nmain: com.example.mcpaneltest.CrashTest\napi-version: '1.21'\n",
    )
    .unwrap();
    let mut cp = Vec::new();
    jars_under(&server.join("versions"), &mut cp);
    jars_under(&server.join("libraries"), &mut cp);
    let cp = std::env::join_paths(cp).unwrap();
    let ok = Command::new(&javac)
        .args(["--release", "21", "-d"])
        .arg(&out)
        .arg("-cp")
        .arg(&cp)
        .arg(src.join("CrashTest.java"))
        .status()
        .unwrap()
        .success();
    assert!(ok, "javac failed");
    let dest = server.join("plugins/CrashTest.jar");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let ok = Command::new(&jar)
        .arg("cf")
        .arg(&dest)
        .arg("-C")
        .arg(&out)
        .arg(".")
        .status()
        .unwrap()
        .success();
    assert!(ok, "jar failed");
    Some(dest)
}

#[tokio::test(flavor = "multi_thread")]
async fn paper_watchdog_crash_names_the_blocking_plugin() {
    if !eula_accepted() {
        return;
    }
    let s = RealServer::create_with("paper", Some("1.21.11"), &[("server-port", "25651")]).await;
    let _guard = KillOnPanic(Arc::clone(&s.core), s.id);
    // First start extracts the server jars and writes spigot.yml.
    s.start_and_wait().await;
    s.stop_and_wait().await;
    let server = s.core.servers.get(s.id).await.unwrap();
    let dir = server.directory.clone();
    let java = s
        .core
        .java
        .get(server.launch.java_runtime_id.unwrap())
        .await
        .unwrap()
        .path;
    let work = tempfile::tempdir().unwrap();
    if build_plugin(&dir, &java, work.path()).is_none() {
        eprintln!("skipped: no javac next to {}", java.display());
        s.finish().await;
        return;
    }
    let spigot = std::fs::read_to_string(dir.join("spigot.yml")).unwrap();
    std::fs::write(
        dir.join("spigot.yml"),
        spigot.replace("  timeout-time: 60", "  timeout-time: 15"),
    )
    .unwrap();
    let mut policy = mcpanel_core::crash::RestartPolicy::default_for(s.id);
    policy.enabled = false;
    s.core.crashes.set_policy(policy, "e2e").await.unwrap();

    s.start_and_wait().await;
    let start = Instant::now();
    let crash = loop {
        if let Some(c) = s.core.crashes.history(s.id, 1).await.unwrap().pop() {
            break c;
        }
        if start.elapsed() > Duration::from_secs(120) {
            let v = s.core.servers.view(s.id).await.unwrap();
            let tail = s.console_tail(4000);
            let interesting: Vec<&str> = tail
                .lines()
                .filter(|l| {
                    l.contains("MCPanel")
                        || l.contains("Stopping")
                        || l.contains("stopped responding")
                        || !l.starts_with('[')
                })
                .collect();
            panic!(
                "no crash recorded; state {:?} exit {:?} diagnosis {:?}\n{}",
                v.runtime.state,
                v.runtime.last_exit_code,
                v.runtime.diagnosis,
                interesting.join("\n")
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    eprintln!(
        "crash: kind={} exit={:?} message={:?}\nanalysis={:?}",
        crash.kind, crash.exit_code, crash.message, crash.analysis
    );
    assert_eq!(crash.kind, "watchdog");
    let first = crash.analysis.suspects.first().expect("a suspect");
    assert_eq!(first.file_name, "CrashTest.jar");
    assert_eq!(first.name, "CrashTest");
    assert_eq!(first.evidence, "loader");
    s.finish().await;
}
