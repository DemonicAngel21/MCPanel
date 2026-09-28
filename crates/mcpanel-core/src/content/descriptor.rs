//! Reads the descriptor inside a plugin or mod jar: `plugin.yml` / `paper-plugin.yml`
//! (Bukkit/Paper), `fabric.mod.json`, `quilt.mod.json`, `META-INF/mods.toml` /
//! `META-INF/neoforge.mods.toml`. A jar without the descriptor its kind requires is
//! rejected before it is installed.

use super::ContentKind;
use crate::error::{CoreError, CoreResult, ErrorCode};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

const MAX_DESCRIPTOR_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    /// Which file described the jar, e.g. `plugin.yml`.
    pub format: String,
    pub name: Option<String>,
    pub version: Option<String>,
}

fn rejected(msg: impl Into<String>) -> CoreError {
    CoreError::new(ErrorCode::ArchiveRejected, msg.into())
}

/// Top-level scalar `key: value` from a YAML document (enough for `name`/`version`;
/// never evaluates anything).
fn yaml_scalar(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        if line.starts_with([' ', '\t', '-', '#']) {
            return None;
        }
        let (k, v) = line.split_once(':')?;
        if k.trim() != key {
            return None;
        }
        let v = v
            .split(" #")
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(['"', '\''])
            .trim();
        (!v.is_empty()).then(|| v.to_string())
    })
}

fn toml_scalar(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        if k.trim() != key {
            return None;
        }
        let v = v.trim().trim_matches('"').trim();
        (!v.is_empty() && !v.starts_with("${")).then(|| v.to_string())
    })
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> CoreResult<Option<String>> {
    let Ok(mut e) = zip.by_name(name) else {
        return Ok(None);
    };
    if e.size() > MAX_DESCRIPTOR_BYTES {
        return Err(rejected(format!("{name} is too large")));
    }
    let mut buf = Vec::new();
    (&mut e)
        .take(MAX_DESCRIPTOR_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| rejected(format!("Cannot read {name}: {e}")))?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Read the descriptor of `jar`. `Ok(None)` = a valid jar without any known descriptor.
pub fn read(jar: &Path) -> CoreResult<Option<Descriptor>> {
    let file = std::fs::File::open(jar).map_err(|e| CoreError::io("Cannot open the jar", &e))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| rejected(format!("Not a valid jar: {e}")))?;
    for yml in ["paper-plugin.yml", "plugin.yml"] {
        if let Some(t) = read_entry(&mut zip, yml)? {
            return Ok(Some(Descriptor {
                format: yml.into(),
                name: yaml_scalar(&t, "name"),
                version: yaml_scalar(&t, "version"),
            }));
        }
    }
    for json in ["fabric.mod.json", "quilt.mod.json"] {
        if let Some(t) = read_entry(&mut zip, json)? {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
            let (name, version) = if json == "quilt.mod.json" {
                let l = &v["quilt_loader"];
                (
                    l["metadata"]["name"].as_str().or(l["id"].as_str()),
                    l["version"].as_str(),
                )
            } else {
                (
                    v["name"].as_str().or(v["id"].as_str()),
                    v["version"].as_str(),
                )
            };
            return Ok(Some(Descriptor {
                format: json.into(),
                name: name.map(String::from),
                version: version.map(String::from),
            }));
        }
    }
    for toml in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(t) = read_entry(&mut zip, toml)? {
            return Ok(Some(Descriptor {
                format: toml.into(),
                name: toml_scalar(&t, "displayName").or_else(|| toml_scalar(&t, "modId")),
                version: toml_scalar(&t, "version"),
            }));
        }
    }
    Ok(None)
}

/// Read and check that the jar is `kind` content.
pub fn validate(jar: &Path, kind: ContentKind) -> CoreResult<Descriptor> {
    let d = read(jar)?
        .ok_or_else(|| rejected("The file is not a plugin or mod (no descriptor found)"))?;
    let is_plugin = d.format.ends_with("plugin.yml");
    match (kind, is_plugin) {
        (ContentKind::Plugin, true) | (ContentKind::Mod, false) => Ok(d),
        (ContentKind::Plugin, false) => Err(rejected("This file is a mod, not a plugin")),
        (ContentKind::Mod, true) => Err(rejected("This file is a plugin, not a mod")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub fn jar(path: &Path, entries: &[(&str, &str)]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (n, c) in entries {
            w.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn reads_plugin_and_mod_descriptors() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("p.jar");
        jar(
            &p,
            &[(
                "plugin.yml",
                "# comment\nname: 'LuckPerms'\nversion: 5.5.71 # build\nmain: me.lucko.Plugin\ncommands:\n  name: nested\n",
            )],
        );
        let desc = validate(&p, ContentKind::Plugin).unwrap();
        assert_eq!(
            (desc.name.as_deref(), desc.version.as_deref()),
            (Some("LuckPerms"), Some("5.5.71"))
        );
        assert_eq!(
            validate(&p, ContentKind::Mod).unwrap_err().code,
            ErrorCode::ArchiveRejected
        );

        let m = d.path().join("m.jar");
        jar(
            &m,
            &[(
                "fabric.mod.json",
                r#"{"id":"sodium","name":"Sodium","version":"0.6.0"}"#,
            )],
        );
        assert_eq!(
            validate(&m, ContentKind::Mod).unwrap().name.as_deref(),
            Some("Sodium")
        );

        let f = d.path().join("f.jar");
        jar(
            &f,
            &[(
                "META-INF/mods.toml",
                "[[mods]]\nmodId=\"jei\"\nversion=\"${file.jarVersion}\"\ndisplayName=\"JEI\"\n",
            )],
        );
        let fd = read(&f).unwrap().unwrap();
        assert_eq!((fd.name.as_deref(), fd.version), (Some("JEI"), None));

        let none = d.path().join("n.jar");
        jar(&none, &[("a.txt", "x")]);
        assert!(read(&none).unwrap().is_none());
        std::fs::write(d.path().join("bad.jar"), b"not a zip").unwrap();
        assert!(read(&d.path().join("bad.jar")).is_err());
    }
}
