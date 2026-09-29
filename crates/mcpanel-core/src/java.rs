//! Java runtime management: discovery (via the platform), validation by executing the
//! runtime, compatibility checks, and persistence.

use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::ids::JavaRuntimeId;
use crate::model::{JavaRuntime, JavaSource};
use crate::ports::{JavaRuntimeRepository, Platform};
use crate::software::JavaRequirement;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JavaCompatibility {
    Compatible,
    /// Will fail to start (`UnsupportedClassVersionError`).
    TooOld {
        required: u32,
    },
    /// Newer than recommended; older game versions may not work.
    NewerThanRecommended {
        recommended: u32,
    },
    NotValidated,
}

pub fn check_compatibility(runtime: &JavaRuntime, req: &JavaRequirement) -> JavaCompatibility {
    if !runtime.valid {
        return JavaCompatibility::NotValidated;
    }
    if runtime.major < req.min_major {
        return JavaCompatibility::TooOld {
            required: req.min_major,
        };
    }
    if let Some(rec) = req.recommended_major
        && runtime.major > rec
        && req.min_major < 17
    {
        // Only old game versions (Java 8–16 era) are known to break on much newer Java.
        return JavaCompatibility::NewerThanRecommended { recommended: rec };
    }
    JavaCompatibility::Compatible
}

/// Parsed `java -XshowSettings:properties -version` output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JavaProbe {
    pub version: String,
    pub major: u32,
    pub vendor: Option<String>,
    pub arch: Option<String>,
    pub is_64bit: bool,
}

pub fn parse_probe(output: &str) -> Option<JavaProbe> {
    let mut props: HashMap<&str, &str> = HashMap::new();
    for line in output.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(" = ") {
            props.insert(k.trim(), v.trim());
        }
    }
    let version = props.get("java.version")?.to_string();
    let spec = props
        .get("java.specification.version")
        .copied()
        .unwrap_or(version.as_str());
    let major = parse_major(spec)?;
    let arch = props.get("os.arch").map(|s| s.to_string());
    let data_model = props.get("sun.arch.data.model").copied();
    let is_64bit = match data_model {
        Some(m) => m == "64",
        None => arch.as_deref().is_some_and(|a| a.contains("64")),
    };
    Some(JavaProbe {
        version,
        major,
        vendor: props
            .get("java.vendor")
            .or_else(|| props.get("java.vm.vendor"))
            .map(|s| s.to_string()),
        arch,
        is_64bit,
    })
}

/// "1.8" → 8, "17" → 17, "21.0.2" → 21.
pub fn parse_major(spec: &str) -> Option<u32> {
    let mut parts = spec.split(['.', '-', '+', '_']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

pub struct JavaManager {
    platform: Arc<dyn Platform>,
    repo: Arc<dyn JavaRuntimeRepository>,
    events: EventBus,
}

fn executable_name_ok(path: &Path) -> bool {
    path.file_name()
        .map(|n| {
            let n = n.to_string_lossy().to_ascii_lowercase();
            n == "java.exe" || n == "java"
        })
        .unwrap_or(false)
}

impl JavaManager {
    pub fn new(
        platform: Arc<dyn Platform>,
        repo: Arc<dyn JavaRuntimeRepository>,
        events: EventBus,
    ) -> Self {
        Self {
            platform,
            repo,
            events,
        }
    }

    pub async fn list(&self) -> CoreResult<Vec<JavaRuntime>> {
        let mut list = self.repo.list().await?;
        list.sort_by(|a, b| b.major.cmp(&a.major).then(a.path.cmp(&b.path)));
        Ok(list)
    }

    pub async fn get(&self, id: JavaRuntimeId) -> CoreResult<JavaRuntime> {
        self.repo
            .get(id)
            .await?
            .ok_or_else(|| CoreError::new(ErrorCode::JavaNotFound, "Java runtime not found"))
    }

    /// Execute the runtime to learn its real version. Never trusts paths or registry data.
    pub async fn probe(&self, path: &Path) -> CoreResult<JavaProbe> {
        if !executable_name_ok(path) {
            return Err(CoreError::new(
                ErrorCode::JavaInvalid,
                "Select a java.exe executable",
            ));
        }
        let md = tokio::fs::metadata(path)
            .await
            .map_err(|_| CoreError::new(ErrorCode::JavaInvalid, "Java executable not found"))?;
        if !md.is_file() {
            return Err(CoreError::new(ErrorCode::JavaInvalid, "Not a file"));
        }
        let out = self
            .platform
            .run_capture(
                path,
                &[
                    "-XshowSettings:properties".to_string(),
                    "-version".to_string(),
                ],
                None,
                Duration::from_secs(20),
            )
            .await?;
        if out.timed_out {
            return Err(CoreError::new(
                ErrorCode::JavaInvalid,
                "Java did not respond in time",
            ));
        }
        // Properties are printed to stderr.
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        parse_probe(&text).ok_or_else(|| {
            CoreError::new(
                ErrorCode::JavaInvalid,
                "Could not determine the Java version",
            )
        })
    }

    async fn validate_into(
        &self,
        path: PathBuf,
        source: JavaSource,
        existing: Option<JavaRuntimeId>,
    ) -> JavaRuntime {
        let now = Timestamp::now();
        match self.probe(&path).await {
            Ok(p) => JavaRuntime {
                id: existing.unwrap_or_default(),
                path,
                major: p.major,
                version: p.version,
                vendor: p.vendor,
                arch: p.arch,
                is_64bit: p.is_64bit,
                source,
                valid: p.is_64bit,
                validation_error: (!p.is_64bit).then(|| {
                    "32-bit Java cannot use more than ~1.5 GB of memory; use a 64-bit runtime"
                        .to_string()
                }),
                validated_at: now,
            },
            Err(e) => JavaRuntime {
                id: existing.unwrap_or_default(),
                path,
                major: 0,
                version: String::new(),
                vendor: None,
                arch: None,
                is_64bit: false,
                source,
                valid: false,
                validation_error: Some(e.message),
                validated_at: now,
            },
        }
    }

    /// Discover candidates, validate them (and re-validate known runtimes), persist.
    pub async fn detect(&self) -> CoreResult<Vec<JavaRuntime>> {
        let known = self.repo.list().await?;
        let mut by_path: HashMap<String, (JavaRuntimeId, JavaSource)> = known
            .iter()
            .map(|r| (r.path.to_string_lossy().to_lowercase(), (r.id, r.source)))
            .collect();
        let platform = Arc::clone(&self.platform);
        let candidates = tokio::task::spawn_blocking(move || platform.java_candidates())
            .await
            .map_err(|e| CoreError::internal(format!("java discovery failed: {e}")))?;
        let mut paths: Vec<(PathBuf, JavaSource, Option<JavaRuntimeId>)> = Vec::new();
        for c in candidates {
            let key = c.to_string_lossy().to_lowercase();
            if let Some((id, src)) = by_path.remove(&key) {
                paths.push((c, src, Some(id)));
            } else if !paths
                .iter()
                .any(|(p, _, _)| p.to_string_lossy().to_lowercase() == key)
            {
                paths.push((c, JavaSource::Detected, None));
            }
        }
        // Known runtimes that were not rediscovered (manual ones, or removed from disk).
        for r in &known {
            let key = r.path.to_string_lossy().to_lowercase();
            if by_path.contains_key(&key) {
                paths.push((r.path.clone(), r.source, Some(r.id)));
            }
        }

        let mut added = Vec::new();
        let results = futures::future::join_all(paths.into_iter().map(|(p, src, id)| async move {
            (id.is_none(), self.validate_into(p, src, id).await)
        }))
        .await;
        for (is_new, rt) in results {
            // Do not persist brand-new candidates that failed validation (noise).
            if is_new && !rt.valid && rt.major == 0 {
                continue;
            }
            let id = self.repo.upsert(&rt).await?;
            if is_new {
                added.push(id);
            }
        }
        if !added.is_empty() {
            self.events
                .publish(DomainEvent::JavaRuntimesChanged { added });
        }
        self.list().await
    }

    /// Register a runtime chosen by the user (path from a native-dialog grant).
    pub async fn add_manual(&self, path: PathBuf) -> CoreResult<JavaRuntime> {
        if !executable_name_ok(&path) {
            return Err(CoreError::new(
                ErrorCode::JavaInvalid,
                "Select a java.exe executable",
            ));
        }
        let existing = self.repo.list().await?.into_iter().find(|r| {
            r.path
                .to_string_lossy()
                .eq_ignore_ascii_case(&path.to_string_lossy())
        });
        let rt = self
            .validate_into(path, JavaSource::Manual, existing.map(|e| e.id))
            .await;
        if !rt.valid && rt.major == 0 {
            return Err(CoreError::new(
                ErrorCode::JavaInvalid,
                rt.validation_error
                    .unwrap_or_else(|| "Invalid Java runtime".into()),
            ));
        }
        let id = self.repo.upsert(&rt).await?;
        self.events
            .publish(DomainEvent::JavaRuntimesChanged { added: vec![id] });
        self.get(id).await
    }

    pub async fn revalidate(&self, id: JavaRuntimeId) -> CoreResult<JavaRuntime> {
        let current = self.get(id).await?;
        let rt = self
            .validate_into(current.path, current.source, Some(id))
            .await;
        self.repo.upsert(&rt).await?;
        self.get(id).await
    }

    pub async fn remove(&self, id: JavaRuntimeId) -> CoreResult<()> {
        self.repo.delete(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_21: &str = r#"Property settings:
    file.encoding = UTF-8
    java.specification.version = 21
    java.vendor = Eclipse Adoptium
    java.version = 21.0.4
    os.arch = amd64
    sun.arch.data.model = 64

openjdk version "21.0.4" 2024-07-16 LTS"#;

    const SAMPLE_8: &str = r#"Property settings:
    java.specification.version = 1.8
    java.vendor = Oracle Corporation
    java.version = 1.8.0_401
    os.arch = x86
    sun.arch.data.model = 32
"#;

    #[test]
    fn parses_modern_and_legacy_output() {
        let p = parse_probe(SAMPLE_21).unwrap();
        assert_eq!(p.major, 21);
        assert_eq!(p.version, "21.0.4");
        assert_eq!(p.vendor.as_deref(), Some("Eclipse Adoptium"));
        assert!(p.is_64bit);
        let p8 = parse_probe(SAMPLE_8).unwrap();
        assert_eq!(p8.major, 8);
        assert!(!p8.is_64bit);
    }

    #[test]
    fn major_parsing() {
        assert_eq!(parse_major("1.8"), Some(8));
        assert_eq!(parse_major("17"), Some(17));
        assert_eq!(parse_major("25-ea"), Some(25));
        assert_eq!(parse_major("x"), None);
    }

    fn rt(major: u32) -> JavaRuntime {
        JavaRuntime {
            id: JavaRuntimeId::new(),
            path: PathBuf::from("java.exe"),
            major,
            version: major.to_string(),
            vendor: None,
            arch: None,
            is_64bit: true,
            source: JavaSource::Detected,
            valid: true,
            validation_error: None,
            validated_at: Timestamp(0),
        }
    }

    #[test]
    fn compatibility_rules() {
        let modern = JavaRequirement {
            min_major: 21,
            recommended_major: Some(21),
            recommended_flags: vec![],
        };
        assert_eq!(
            check_compatibility(&rt(17), &modern),
            JavaCompatibility::TooOld { required: 21 }
        );
        assert_eq!(
            check_compatibility(&rt(25), &modern),
            JavaCompatibility::Compatible
        );
        let old = JavaRequirement {
            min_major: 8,
            recommended_major: Some(8),
            recommended_flags: vec![],
        };
        assert_eq!(
            check_compatibility(&rt(21), &old),
            JavaCompatibility::NewerThanRecommended { recommended: 8 }
        );
    }
}
