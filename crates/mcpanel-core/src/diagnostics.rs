//! Crash analysis: the root-cause exception and the plugins/mods whose code appears in
//! the crashing thread. Verified 2026-09-29 against a real Paper 1.21.11 watchdog dump:
//! frames look like `CrashTest.jar//com.example.mcpaneltest.CrashTest.run(CrashTest.java:14)`
//! — the JVM prefixes frames with the class loader's name, which Paper sets to the
//! plugin jar. Where no loader name is printed (mod loaders, vanilla crash reports), a
//! frame is attributed by class package to the one installed jar that contains it.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suspect {
    /// Display name (from the jar's descriptor), or the file name.
    pub name: String,
    pub file_name: String,
    /// How it was identified: `loader` (the frame names the jar) or `package`.
    pub evidence: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashAnalysis {
    /// Root cause, e.g. `java.lang.IllegalStateException: boom`.
    pub exception: Option<String>,
    /// Plugins/mods in the crashing code path, most likely first.
    pub suspects: Vec<Suspect>,
}

/// An installed jar the analysis may blame.
#[derive(Debug, Clone)]
pub struct JarInfo {
    pub file_name: String,
    pub name: String,
    pub path: PathBuf,
}

static FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|\sat |\s)(?:(?P<loader>[^\s/]+\.jar)//)?(?:[\w.-]+@[\w.-]+/)?(?P<class>[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)+)\.[\w$<>-]+\((?:[^)]*)\)\s*$",
    )
    .expect("regex")
});
static EXCEPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Caused by: |Exception in thread .*?\s)?(?P<ex>(?:[a-z_$][\w$]*\.)+[A-Z][\w$]*(?:Exception|Error|Throwable))(?::\s*(?P<msg>.*))?$")
        .expect("regex")
});

/// Code that is never a suspect: the JDK, Minecraft, server software and loaders.
const PLATFORM_PREFIXES: &[&str] = &[
    "java.",
    "javax.",
    "jdk.",
    "sun.",
    "com.sun.",
    "net.minecraft.",
    "com.mojang.",
    "org.bukkit.",
    "org.spigotmc.",
    "io.papermc.",
    "com.destroystokyo.",
    "ca.spottedleaf.",
    "net.neoforged.",
    "net.minecraftforge.",
    "cpw.mods.",
    "net.fabricmc.",
    "org.quiltmc.",
    "org.spongepowered.asm.",
    "org.purpurmc.",
    "io.netty.",
    "org.apache.",
    "com.google.",
    "it.unimi.",
    "org.slf4j.",
    "org.objectweb.",
];

/// Package prefixes that libraries are commonly shaded under (never attributed).
const LIBRARY_PREFIXES: &[&str] = &[
    "kotlin",
    "kotlinx",
    "org/intellij",
    "org/jetbrains",
    "net/kyori",
    "org/bstats",
    "com/zaxxer",
    "org/yaml",
    "com/fasterxml",
    "org/checkerframework",
    "javax",
    "org/json",
    "okhttp3",
    "okio",
    "org/slf4j",
    "io/netty",
    "org/apache",
    "com/google",
    "it/unimi",
    "org/objectweb",
    "org/xerial",
    "org/mariadb",
    "com/mysql",
    "org/h2",
    "org/sqlite",
    "redis",
    "io/leangen",
];

fn is_platform(class: &str) -> bool {
    PLATFORM_PREFIXES.iter().any(|p| class.starts_with(p))
}

/// The message part of a console line (log prefix and 26.x "System chat: " removed).
fn msg(line: &str) -> String {
    crate::console::dialect::parse_line(line).message
}

/// Frames of the relevant thread: Paper's "Server thread" dump when present, else all.
fn relevant_lines(lines: &[String]) -> Vec<String> {
    let msgs: Vec<String> = lines.iter().map(|l| msg(l)).collect();
    if let Some(start) = msgs
        .iter()
        .rposition(|m| m.trim() == "Current Thread: Server thread")
    {
        let end = msgs[start + 1..]
            .iter()
            .position(|m| m.trim().starts_with("Current Thread:") || m.trim().starts_with("-----"))
            .map_or(msgs.len(), |p| start + 1 + p);
        return msgs[start..end].to_vec();
    }
    msgs
}

/// Top-level package prefixes (`a/b/c`) of the classes in a jar, without shaded
/// libraries. Reads only the zip directory.
pub fn jar_packages(path: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(f) = std::fs::File::open(path) else {
        return out;
    };
    let Ok(mut z) = zip::ZipArchive::new(f) else {
        return out;
    };
    for i in 0..z.len().min(50_000) {
        let Ok(e) = z.by_index_raw(i) else { continue };
        let name = e.name();
        if !name.ends_with(".class") || name.starts_with("META-INF/") {
            continue;
        }
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() < 3 {
            continue;
        }
        let prefix = parts[..3.min(parts.len() - 1)].join("/");
        if LIBRARY_PREFIXES.iter().any(|l| prefix.starts_with(l)) {
            continue;
        }
        out.insert(prefix);
    }
    out
}

/// Analyse crash output (`lines`: crash report and/or console lines).
pub fn analyze(lines: &[String], jars: &[JarInfo]) -> CrashAnalysis {
    // Root cause: the last "Caused by", else the first exception line.
    let msgs: Vec<String> = lines.iter().map(|l| msg(l).trim().to_string()).collect();
    let exceptions: Vec<(bool, String)> = msgs
        .iter()
        .filter_map(|m| {
            let c = EXCEPTION.captures(m)?;
            let ex = c.name("ex")?.as_str();
            let text = match c
                .name("msg")
                .map(|m| m.as_str().trim())
                .filter(|m| !m.is_empty())
            {
                Some(msg) => format!("{ex}: {msg}"),
                None => ex.to_string(),
            };
            Some((m.starts_with("Caused by: "), text))
        })
        .collect();
    let exception = exceptions
        .iter()
        .rev()
        .find(|(caused, _)| *caused)
        .or_else(|| exceptions.first())
        .map(|(_, t)| t.chars().take(300).collect());

    // Package index, only for prefixes unique to one jar.
    let mut owners: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut indexed = false;
    let by_file = |f: &str| {
        jars.iter()
            .position(|j| j.file_name.eq_ignore_ascii_case(f))
    };
    let mut suspects: Vec<Suspect> = Vec::new();
    let push = |i: usize, evidence: &str, suspects: &mut Vec<Suspect>| {
        if !suspects.iter().any(|s| s.file_name == jars[i].file_name) {
            suspects.push(Suspect {
                name: jars[i].name.clone(),
                file_name: jars[i].file_name.clone(),
                evidence: evidence.into(),
            });
        }
    };
    for line in relevant_lines(lines) {
        let Some(c) = FRAME.captures(line.trim_end()) else {
            continue;
        };
        let class = c.name("class").map_or("", |m| m.as_str());
        if let Some(loader) = c.name("loader")
            && let Some(i) = by_file(loader.as_str())
        {
            push(i, "loader", &mut suspects);
            continue;
        }
        if is_platform(class) {
            continue;
        }
        if !indexed {
            for (i, j) in jars.iter().enumerate() {
                for p in jar_packages(&j.path) {
                    owners.entry(p).or_default().push(i);
                }
            }
            indexed = true;
        }
        let path = class.replace('.', "/");
        let segs: Vec<&str> = path.split('/').collect();
        for n in (2..=3.min(segs.len().saturating_sub(1))).rev() {
            let prefix = segs[..n].join("/");
            if let Some(o) = owners.get(&prefix)
                && o.len() == 1
            {
                push(o[0], "package", &mut suspects);
                break;
            }
        }
        if suspects.len() >= 5 {
            break;
        }
    }
    CrashAnalysis {
        exception,
        suspects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(dir: &Path, name: &str, classes: &[&str]) -> JarInfo {
        let path = dir.join(name);
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for c in classes {
            z.start_file(*c, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b"x").unwrap();
        }
        z.finish().unwrap();
        JarInfo {
            file_name: name.into(),
            name: name.trim_end_matches(".jar").into(),
            path,
        }
    }

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn paper_watchdog_dump_names_the_plugin_jar() {
        let d = tempfile::tempdir().unwrap();
        let jars = vec![jar(
            d.path(),
            "CrashTest.jar",
            &["com/example/mcpaneltest/CrashTest.class"],
        )];
        let dump = lines(
            "[16:27:24 ERROR]: The server has stopped responding! This is (probably) not a Paper bug.\n\
             [16:27:24 ERROR]: Current Thread: Server thread\n\
             [16:27:24 ERROR]: \tPID: 62 | Suspended: false | Native: false | State: TIMED_WAITING\n\
             [16:27:24 ERROR]: \tStack:\n\
             [16:27:24 ERROR]: \t\tjava.base@26.0.1/java.lang.Thread.sleep(Thread.java:582)\n\
             [16:27:24 ERROR]: \t\tCrashTest.jar//com.example.mcpaneltest.CrashTest.blockServerThread(CrashTest.java:14)\n\
             [16:27:24 ERROR]: \t\torg.bukkit.craftbukkit.scheduler.CraftTask.run(CraftTask.java:78)\n\
             [16:27:24 ERROR]: ------------------------------\n\
             [16:27:24 ERROR]: Current Thread: Reference Handler\n\
             [16:27:24 ERROR]: \t\tOther.jar//org.other.Thing.run(Thing.java:1)",
        );
        let a = analyze(&dump, &jars);
        assert_eq!(a.suspects.len(), 1);
        assert_eq!(a.suspects[0].file_name, "CrashTest.jar");
        assert_eq!(a.suspects[0].evidence, "loader");
    }

    #[test]
    fn exception_traces_are_attributed_by_unique_package() {
        let d = tempfile::tempdir().unwrap();
        let jars = vec![
            jar(
                d.path(),
                "cool-mod.jar",
                &["dev/cool/mod/Entry.class", "kotlin/Unit.class"],
            ),
            jar(
                d.path(),
                "other.jar",
                &["dev/other/x/A.class", "kotlin/Unit.class"],
            ),
        ];
        let trace = lines(
            "[12:00:00] [Server thread/ERROR]: Encountered an unexpected exception\n\
             java.lang.RuntimeException: Ticking entity\n\
             \tat net.minecraft.server.MinecraftServer.tick(MinecraftServer.java:1)\n\
             \tat kotlin.Unit.run(Unit.kt:1)\n\
             \tat dev.cool.mod.Entry.onTick(Entry.java:42)\n\
             Caused by: java.lang.NullPointerException: Cannot invoke \"x\" because \"y\" is null\n\
             \tat dev.cool.mod.Entry.inner(Entry.java:50)",
        );
        let a = analyze(&trace, &jars);
        assert_eq!(
            a.exception.as_deref(),
            Some("java.lang.NullPointerException: Cannot invoke \"x\" because \"y\" is null")
        );
        assert_eq!(a.suspects.len(), 1, "{:?}", a.suspects);
        assert_eq!(a.suspects[0].file_name, "cool-mod.jar");
        assert_eq!(a.suspects[0].evidence, "package");
    }

    #[test]
    fn platform_only_crashes_have_no_suspects() {
        let a = analyze(
            &lines(
                "java.lang.IllegalStateException: boom\n\tat net.minecraft.Fake.tick(Fake.java:1)",
            ),
            &[],
        );
        assert_eq!(
            a.exception.as_deref(),
            Some("java.lang.IllegalStateException: boom")
        );
        assert!(a.suspects.is_empty());
    }
}

// ───────────────────────────── support bundle ─────────────────────────────

/// What goes into a diagnostics bundle besides files read from disk.
#[derive(Debug, Clone, Serialize)]
pub struct BundleInfo {
    pub generator: String,
    pub created_at: crate::time::Timestamp,
    pub os: String,
    pub server: serde_json::Value,
    pub java: serde_json::Value,
    pub content: Vec<serde_json::Value>,
    pub crashes: Vec<serde_json::Value>,
}

/// Inputs for [`write_bundle`].
pub struct BundleSources {
    pub server_dir: PathBuf,
    pub console_dir: PathBuf,
    pub app_logs_dir: PathBuf,
    /// server.properties keys whose values are secret.
    pub sensitive_properties: Vec<String>,
}

const MAX_LOG_BYTES: u64 = 20 * 1024 * 1024;

/// Whether `dir` is a real folder (links/junctions are not followed into).
fn real_dir(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir)
        .is_ok_and(|md| md.is_dir() && !crate::files::fsx::is_reparse_point(&md))
}

fn newest_files(dir: &Path, filter: impl Fn(&str) -> bool, n: usize) -> Vec<PathBuf> {
    if !real_dir(dir) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let md = std::fs::symlink_metadata(e.path()).ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            (md.is_file() && filter(&name)).then(|| Some((md.modified().ok()?, e.path())))?
        })
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.0));
    v.into_iter().take(n).map(|(_, p)| p).collect()
}

/// `server.properties` with secret values replaced (parsed with the real properties
/// parser, so every separator form and line continuations are handled).
pub fn redact_properties(text: &str, sensitive: &[String]) -> String {
    let mut doc = crate::config::PropertiesDocument::parse(text);
    for k in sensitive {
        if doc.get(k).is_some_and(|v| !v.trim().is_empty()) {
            let _ = doc.set(k, "<redacted>");
        }
    }
    doc.to_text()
}

fn file_name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Write a support ZIP: summary, the server's latest log and crash reports, MCPanel's
/// console captures and app logs, and a redacted `server.properties`. Never includes
/// worlds or files classified as sensitive. Returns the number of entries.
pub fn write_bundle(
    dest: &Path,
    info: &BundleInfo,
    src: &BundleSources,
) -> crate::error::CoreResult<usize> {
    use crate::error::CoreError;
    use std::io::{Read, Write};
    let io = |e: std::io::Error| CoreError::io("Cannot write the diagnostics file", &e);
    let zerr =
        |e: zip::result::ZipError| CoreError::new(crate::error::ErrorCode::Io, e.to_string());
    let file = std::fs::File::create(dest).map_err(io)?;
    let mut z = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut count = 0usize;

    z.start_file("mcpanel-diagnostics.json", opts)
        .map_err(zerr)?;
    z.write_all(
        serde_json::to_string_pretty(info)
            .map_err(|e| CoreError::internal(e.to_string()))?
            .as_bytes(),
    )
    .map_err(io)?;
    count += 1;

    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let normal = |rel: &str| -> bool {
        crate::files::safepath::parse_relative(rel).is_ok_and(|c| {
            crate::files::sensitivity::classify(&c)
                == crate::files::sensitivity::Sensitivity::Normal
        })
    };
    if normal("logs/latest.log") && real_dir(&src.server_dir.join("logs")) {
        files.push((
            "server/logs/latest.log".into(),
            src.server_dir.join("logs/latest.log"),
        ));
    }
    for p in newest_files(
        &src.server_dir.join("crash-reports"),
        |n| n.ends_with(".txt"),
        3,
    ) {
        let name = file_name_of(&p);
        if normal(&format!("crash-reports/{name}")) {
            files.push((format!("server/crash-reports/{name}"), p));
        }
    }
    for p in newest_files(&src.console_dir, |n| n.ends_with(".log"), 2) {
        files.push((format!("mcpanel/console/{}", file_name_of(&p)), p));
    }
    for p in newest_files(&src.app_logs_dir, |_| true, 2) {
        files.push((format!("mcpanel/logs/{}", file_name_of(&p)), p));
    }
    for (name, path) in files {
        let Ok(md) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !md.is_file() || crate::files::fsx::is_reparse_point(&md) {
            continue;
        }
        let Ok(mut f) = std::fs::File::open(&path) else {
            continue;
        };
        z.start_file(name, opts).map_err(zerr)?;
        // Logs can be huge: keep the newest part.
        if md.len() > MAX_LOG_BYTES {
            use std::io::Seek;
            f.seek(std::io::SeekFrom::Start(md.len() - MAX_LOG_BYTES))
                .map_err(io)?;
            z.write_all(b"[... earlier output omitted ...]\n")
                .map_err(io)?;
        }
        std::io::copy(&mut (&mut f).take(MAX_LOG_BYTES), &mut z).map_err(io)?;
        count += 1;
    }
    let props = src.server_dir.join("server.properties");
    if let Ok(md) = std::fs::symlink_metadata(&props)
        && md.is_file()
        && md.len() < 1024 * 1024
        && let Ok(bytes) = std::fs::read(&props)
    {
        let text = crate::config::properties::decode_bytes(&bytes);
        z.start_file("server/server.properties", opts)
            .map_err(zerr)?;
        z.write_all(redact_properties(&text, &src.sensitive_properties).as_bytes())
            .map_err(io)?;
        count += 1;
    }
    z.finish().map_err(zerr)?;
    Ok(count)
}

#[cfg(test)]
mod bundle_tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn secrets_are_redacted() {
        let keys = ["rcon.password".to_string()];
        let r = redact_properties(
            "#c\nrcon.password=hunter2\nmotd=Hi\nlevel-name=world",
            &keys,
        );
        assert!(r.contains("rcon.password=<redacted>") && !r.contains("hunter2"));
        assert!(r.contains("motd=Hi"));
        // Whitespace separator and line continuations.
        let r = redact_properties("rcon.password hunter2\nmotd=x", &keys);
        assert!(!r.contains("hunter2"), "{r}");
        let r = redact_properties("rcon.password=hun\\\n  ter2\nmotd=x", &keys);
        assert!(!r.contains("hun") && !r.contains("ter2"), "{r}");
    }

    #[test]
    fn bundle_holds_logs_and_never_worlds_or_keys() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("srv");
        std::fs::create_dir_all(s.join("logs")).unwrap();
        std::fs::create_dir_all(s.join("crash-reports")).unwrap();
        std::fs::create_dir_all(s.join("world")).unwrap();
        std::fs::create_dir_all(s.join("plugins/floodgate")).unwrap();
        std::fs::write(s.join("logs/latest.log"), "hello log").unwrap();
        std::fs::write(s.join("crash-reports/crash-1.txt"), "Description: boom").unwrap();
        std::fs::write(s.join("world/level.dat"), "w").unwrap();
        std::fs::write(s.join("plugins/floodgate/key.pem"), "secret").unwrap();
        std::fs::write(s.join("server.properties"), "rcon.password=pw\nmotd=x\n").unwrap();
        let console = d.path().join("console");
        std::fs::create_dir_all(&console).unwrap();
        std::fs::write(console.join("console-1.log"), "c").unwrap();
        let out = d.path().join("diag.zip");
        let info = BundleInfo {
            generator: "test".into(),
            created_at: crate::time::Timestamp(1),
            os: "Windows".into(),
            server: serde_json::json!({ "name": "x" }),
            java: serde_json::Value::Null,
            content: vec![],
            crashes: vec![],
        };
        let n = write_bundle(
            &out,
            &info,
            &BundleSources {
                server_dir: s,
                console_dir: console,
                app_logs_dir: d.path().join("nologs"),
                sensitive_properties: vec!["rcon.password".into()],
            },
        )
        .unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let names: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(n, names.len());
        for want in [
            "mcpanel-diagnostics.json",
            "server/logs/latest.log",
            "server/crash-reports/crash-1.txt",
            "mcpanel/console/console-1.log",
            "server/server.properties",
        ] {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
        assert!(
            !names
                .iter()
                .any(|n| n.contains("world") || n.contains("key.pem"))
        );
        let mut props = String::new();
        z.by_name("server/server.properties")
            .unwrap()
            .read_to_string(&mut props)
            .unwrap();
        assert!(props.contains("<redacted>") && !props.contains("=pw"));
    }
}
