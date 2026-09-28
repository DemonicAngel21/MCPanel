//! `mcpanel-seed`: prepare an isolated MCPanel data directory with a fake-mc server,
//! for UI end-to-end testing without real Minecraft downloads or EULA acceptance.
//!
//! Usage: mcpanel-seed <data-dir> <servers-dir> <fake-mc.exe>
//! Then run the app with MCPANEL_DATA_DIR/MCPANEL_SERVERS_DIR pointing at the same dirs.
#![allow(clippy::unwrap_used)]

use mcpanel_core::ids::{JavaRuntimeId, ServerId};
use mcpanel_core::model::{InstalledSoftware, JavaRuntime, JavaSource, LaunchConfig, Server};
use mcpanel_core::time::Timestamp;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: mcpanel-seed <data-dir> <servers-dir> <fake-mc.exe>");
        std::process::exit(2);
    }
    let data = PathBuf::from(&args[1]);
    let servers = PathBuf::from(&args[2]);
    let fake = PathBuf::from(&args[3]);

    let jdk = data.join("fake-jdk").join("bin");
    std::fs::create_dir_all(&jdk).unwrap();
    let java = jdk.join("java.exe");
    std::fs::copy(&fake, &java).unwrap();

    let dir = servers.join("fake-smp");
    std::fs::create_dir_all(dir.join("plugins").join("floodgate")).unwrap();
    std::fs::write(dir.join("server.jar"), b"fake-mc does not read this file").unwrap();
    // A fake server (not Mojang software): the test fixture itself provides eula.txt.
    std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();
    std::fs::write(
        dir.join("server.properties"),
        "#Minecraft server properties\nmotd=\\u00A7aFake SMP\nserver-port=25599\ndifficulty=easy\ngamemode=survival\npvp=true\nview-distance=10\nmax-players=20\nonline-mode=true\nwhite-list=false\nlevel-name=world\nrcon.password=hunter2\nspawn-protection=16\n",
    )
    .unwrap();
    std::fs::write(dir.join("fake-mc.json"), r#"{"startup_ms": 1500}"#).unwrap();
    std::fs::write(
        dir.join("plugins").join("floodgate").join("key.pem"),
        b"fake key material",
    )
    .unwrap();
    std::fs::write(
        dir.join("plugins").join("floodgate").join("config.yml"),
        "username-prefix: \".\"\nreplace-spaces: true\n",
    )
    .unwrap();

    let (db, _) = mcpanel_db::Database::open(&data.join("mcpanel.db"))
        .await
        .unwrap();
    let repos = db.repositories();
    let jid = repos
        .java
        .upsert(&JavaRuntime {
            id: JavaRuntimeId::new(),
            path: java,
            major: 21,
            version: "21.0.0".into(),
            vendor: Some("MCPanel Test Double".into()),
            arch: Some("amd64".into()),
            is_64bit: true,
            source: JavaSource::Manual,
            valid: true,
            validation_error: None,
            validated_at: Timestamp::now(),
        })
        .await
        .unwrap();
    let now = Timestamp::now();
    repos
        .servers
        .insert(&Server {
            id: ServerId::new(),
            name: "Fake SMP".into(),
            directory: dir,
            software: InstalledSoftware {
                software_id: "paper".into(),
                game_version: "1.21.11".into(),
                build: Some("132".into()),
                jar: "server.jar".into(),
                java_min_major: Some(21),
                java_recommended_major: None,
            },
            launch: LaunchConfig {
                java_runtime_id: Some(jid),
                min_memory_mb: 512,
                max_memory_mb: 1024,
                jvm_args: vec![],
                server_args: vec![],
                stop_timeout_secs: 30,
            },
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    println!("seeded");
}
