//! `fake-mc`: a test double for "java running a Minecraft server".
//!
//! - Invoked with `-XshowSettings:properties -version` it prints Java-like properties
//!   to stderr (so MCPanel's Java validation accepts it as Java 21, 64-bit).
//! - Otherwise it behaves like a Minecraft server: prints vanilla-style log lines,
//!   reports "Done", and reacts to stdin commands. Behaviour is configured by an
//!   optional `fake-mc.json` in the working directory (the environment is cleared by
//!   MCPanel, so files are the only channel).
//!
//! Commands: `stop`, `crash`, `flood <n>`, `join <name>`, `leave <name>`, `say <text>`,
//! `oom`, `hang` (stop reading stdin), `save-off`, `save-all [flush]`, `save-on`, and
//! replies (only) to `op`, `deop`, `whitelist`, `ban`, `pardon`, `kick`; `watchdog` exits
//! like a hung server, and `crash` writes a crash report with `"crash_report": true`.
//!
//! With `"world": true` it keeps `world/session.lock` locked like a real server, and
//! `save-all` writes a save counter to `world/level.dat`.

use serde::Deserialize;
use std::io::{BufRead, Write};
use std::time::Duration;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Config {
    startup_ms: u64,
    ignore_stop: bool,
    fail_bind: bool,
    exit_immediately_code: Option<i32>,
    java_major: Option<u32>,
    world: bool,
    save_delay_ms: u64,
    /// Log command feedback and join/leave as "System chat: …" like Minecraft 26.x.
    system_chat: bool,
    /// `crash` also writes a vanilla-style `crash-reports/` file.
    crash_report: bool,
    /// Behave like a server without the performance commands (they are unknown).
    no_perf_commands: bool,
}

fn log(level: &str, msg: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "[12:00:00] [Server thread/{level}]: {msg}");
    let _ = out.flush();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg: Config = std::fs::read_to_string("fake-mc.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if args.iter().any(|a| a == "-version") {
        let major = cfg.java_major.unwrap_or(21);
        eprintln!("Property settings:");
        eprintln!("    java.specification.version = {major}");
        eprintln!("    java.vendor = MCPanel Test Double");
        eprintln!("    java.version = {major}.0.0");
        eprintln!("    os.arch = amd64");
        eprintln!("    sun.arch.data.model = 64");
        eprintln!();
        eprintln!("openjdk version \"{major}.0.0\"");
        return;
    }

    if let Some(code) = cfg.exit_immediately_code {
        eprintln!("Error: Unable to access jarfile server.jar");
        std::process::exit(code);
    }

    log("INFO", "Starting minecraft server version fake");
    log("INFO", "Loading properties");
    if cfg.fail_bind {
        log("WARN", "**** FAILED TO BIND TO PORT!");
        log(
            "WARN",
            "The exception was: java.net.BindException: Address already in use: bind",
        );
        std::process::exit(1);
    }
    let _session_lock = cfg.world.then(|| {
        std::fs::create_dir_all("world").ok();
        let f = std::fs::File::create("world/session.lock").ok()?;
        f.lock().ok()?;
        Some(f)
    });
    let mut saves = 0u32;
    let chat = |msg: &str| {
        if cfg.system_chat {
            log("INFO", &format!("System chat: {msg}"));
        } else {
            log("INFO", msg);
        }
    };
    std::thread::sleep(Duration::from_millis(cfg.startup_ms));
    log("INFO", "Done (1.234s)! For help, type \"help\"");

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim().to_string();
        let (cmd, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        match cmd {
            "stop" => {
                if cfg.ignore_stop {
                    log("INFO", "Ignoring stop (test mode)");
                    continue;
                }
                log("INFO", "Stopping the server");
                log("INFO", "Saving worlds");
                std::process::exit(0);
            }
            // Paper-style performance commands and vanilla `tick query`.
            "tps" | "mspt" | "tick" if cfg.no_perf_commands => {
                log("INFO", &format!("Unknown command: {cmd}"))
            }
            "tps" => log("INFO", "TPS from last 1m, 5m, 15m: *20.0, 19.5, 19.0"),
            "mspt" => {
                log(
                    "INFO",
                    "Server tick times (avg/min/max) from last 5s, 10s, 1m:",
                );
                log("INFO", "\u{25f4} 2.5/1.0/9.5, 2.0/1.0/9.5, 2.0/1.0/9.5");
            }
            "tick" if rest == "query" => {
                log("INFO", "The game is running normally");
                log("INFO", "Target tick rate: 20.0 per second.");
                println!("Average time per tick: 2.5ms (Target: 50.0ms)");
                log(
                    "INFO",
                    "Percentiles: P50: 2.0ms P95: 4.0ms P99: 9.0ms. Sample: 100",
                );
            }
            "watchdog" => {
                log(
                    "ERROR",
                    "A single server tick took 60.00 seconds (should be max 0.05)",
                );
                log(
                    "ERROR",
                    "Considering it to be crashed, server will forcibly shutdown.",
                );
                std::process::exit(1);
            }
            "crash" => {
                if cfg.crash_report {
                    let _ = std::fs::create_dir_all("crash-reports");
                    let _ = std::fs::write(
                        "crash-reports/crash-2026-09-28_12.00.00-server.txt",
                        "---- Minecraft Crash Report ----
// Why did you do that?

Time: 2026-09-28 12:00:00
Description: Exception in server tick loop

java.lang.IllegalStateException: boom
",
                    );
                }
                eprintln!(
                    "Exception in thread \"Server thread\" java.lang.IllegalStateException: boom"
                );
                eprintln!("\tat net.minecraft.Fake.tick(Fake.java:1)");
                std::process::exit(1);
            }
            "oom" => {
                log("ERROR", "java.lang.OutOfMemoryError: Java heap space");
                std::process::exit(3);
            }
            "flood" => {
                let n: usize = rest.parse().unwrap_or(1000);
                let mut out = std::io::stdout().lock();
                for i in 0..n {
                    let _ = writeln!(out, "[12:00:00] [Server thread/INFO]: flood line {i}");
                }
                let _ = writeln!(out, "[12:00:00] [Server thread/INFO]: flood done");
                let _ = out.flush();
            }
            "join" => chat(&format!("{rest} joined the game")),
            "leave" => chat(&format!("{rest} left the game")),
            "op" => chat(&format!("Made {rest} a server operator")),
            "deop" => chat(&format!("Made {rest} no longer a server operator")),
            "whitelist" => chat(&format!("Whitelist: {rest}")),
            "ban" => chat(&format!("Banned {rest}")),
            "pardon" => chat(&format!("Unbanned {rest}")),
            "kick" => {
                let (who, why) = rest
                    .split_once(' ')
                    .unwrap_or((rest, "Kicked by an operator"));
                chat(&format!("Kicked {who}: {why}"));
                chat(&format!("{who} left the game"));
            }
            "say" => log("INFO", &format!("[Server] {rest}")),
            "save-off" => log("INFO", "Automatic saving is now disabled"),
            "save-on" => log("INFO", "Automatic saving is now enabled"),
            "save-all" => {
                log("INFO", "Saving the game (this may take a moment!)");
                std::thread::sleep(Duration::from_millis(cfg.save_delay_ms));
                if cfg.world {
                    saves += 1;
                    let _ = std::fs::write("world/level.dat", format!("saves={saves}"));
                }
                log("INFO", "Saved the game");
            }
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            "" => {}
            other => log("INFO", &format!("Unknown command: {other}")),
        }
    }
    // stdin closed: a real server keeps running; emulate by idling until killed.
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
