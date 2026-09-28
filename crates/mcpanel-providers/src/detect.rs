//! Detection of existing server installations (import). Jar contents are untrusted and
//! read with size limits.

use mcpanel_core::software::DetectedSoftware;
use mcpanel_core::software::jar::{jar_has_entry_prefix, read_jar_entry, root_jars};
use std::path::Path;

/// `<prefix>-<version>-<build>.jar` → (version, build).
pub(crate) fn parse_prefixed_name(name: &str, prefix: &str) -> Option<(String, Option<String>)> {
    let lower = name.to_ascii_lowercase();
    let rest = lower
        .strip_prefix(&format!("{prefix}-"))?
        .strip_suffix(".jar")?;
    let rest = &name[prefix.len() + 1..prefix.len() + 1 + rest.len()];
    match rest.rsplit_once('-') {
        Some((version, build))
            if build.chars().all(|c| c.is_ascii_digit()) && !version.is_empty() =>
        {
            Some((version.to_string(), Some(build.to_string())))
        }
        _ => Some((rest.to_string(), None)),
    }
}

fn version_json_id(jar: &Path) -> Option<String> {
    let bytes = read_jar_entry(jar, "version.json", 64 * 1024)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("id")?.as_str().map(str::to_string)
}

pub(crate) fn detect_prefixed(
    dir: &Path,
    prefix: &str,
    software_id: &str,
) -> Option<DetectedSoftware> {
    for (name, _) in root_jars(dir) {
        if let Some((version, build)) = parse_prefixed_name(&name, prefix) {
            return Some(DetectedSoftware {
                software_id: software_id.into(),
                game_version: Some(version),
                build,
                jar: name,
                confidence: 90,
            });
        }
    }
    if software_id == "paper" {
        // Renamed Paper jars still contain the Paperclip launcher.
        for (name, _) in root_jars(dir) {
            let jar = dir.join(&name);
            if jar_has_entry_prefix(&jar, "io/papermc/paperclip/") {
                return Some(DetectedSoftware {
                    software_id: "paper".into(),
                    game_version: version_json_id(&jar),
                    build: None,
                    jar: name,
                    confidence: 60,
                });
            }
        }
    }
    None
}

pub(crate) fn detect_vanilla(dir: &Path) -> Option<DetectedSoftware> {
    for (name, _) in root_jars(dir) {
        let lower = name.to_ascii_lowercase();
        if let Some(v) = lower
            .strip_prefix("minecraft_server.")
            .and_then(|r| r.strip_suffix(".jar"))
        {
            let version = name["minecraft_server.".len()..name.len() - 4].to_string();
            let _ = v;
            return Some(DetectedSoftware {
                software_id: "vanilla".into(),
                game_version: Some(version),
                build: None,
                jar: name,
                confidence: 80,
            });
        }
    }
    for (name, _) in root_jars(dir) {
        let jar = dir.join(&name);
        if jar_has_entry_prefix(&jar, "io/papermc/") {
            continue;
        }
        if jar_has_entry_prefix(&jar, "net/minecraft/bundler/")
            || jar_has_entry_prefix(&jar, "net/minecraft/server/")
        {
            return Some(DetectedSoftware {
                software_id: "vanilla".into(),
                game_version: version_json_id(&jar),
                build: None,
                jar: name,
                confidence: 50,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn prefixed_names() {
        assert_eq!(
            parse_prefixed_name("paper-1.21.11-132.jar", "paper"),
            Some(("1.21.11".into(), Some("132".into())))
        );
        assert_eq!(
            parse_prefixed_name("purpur-1.21.11-2568.jar", "purpur"),
            Some(("1.21.11".into(), Some("2568".into())))
        );
        assert_eq!(
            parse_prefixed_name("Paper-26.1.2-rc-3.jar", "paper"),
            Some(("26.1.2-rc".into(), Some("3".into())))
        );
        assert_eq!(parse_prefixed_name("server.jar", "paper"), None);
    }

    fn jar(dir: &Path, name: &str, entries: &[(&str, &[u8])]) {
        let f = std::fs::File::create(dir.join(name)).unwrap();
        let mut w = zip::ZipWriter::new(f);
        for (n, d) in entries {
            w.start_file(n.to_string(), zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn detects_vanilla_bundler_by_contents() {
        let d = tempfile::tempdir().unwrap();
        jar(
            d.path(),
            "server.jar",
            &[
                ("net/minecraft/bundler/Main.class", b"x"),
                ("version.json", br#"{"id":"1.21.4"}"#),
            ],
        );
        let det = detect_vanilla(d.path()).unwrap();
        assert_eq!(det.game_version.as_deref(), Some("1.21.4"));
        assert_eq!(det.jar, "server.jar");
    }

    #[test]
    fn detects_renamed_paper() {
        let d = tempfile::tempdir().unwrap();
        jar(
            d.path(),
            "server.jar",
            &[
                ("io/papermc/paperclip/Main.class", b"x"),
                ("version.json", br#"{"id":"1.20.1"}"#),
            ],
        );
        assert!(detect_vanilla(d.path()).is_none());
        let det = detect_prefixed(d.path(), "paper", "paper").unwrap();
        assert_eq!(det.game_version.as_deref(), Some("1.20.1"));
    }
}
