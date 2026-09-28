//! A deterministic content provider ("TestHub") and in-memory downloader for pipeline
//! tests. The downloader verifies hashes exactly like the real one, so hash-mismatch
//! handling is exercised.

use async_trait::async_trait;
use mcpanel_core::content::{
    ContentDependency, ContentFile, ContentKind, ContentProvider, ContentProviderInfo,
    ContentTarget, ContentVersion, DependencyKind, ProjectSummary, ReleaseChannel, SearchPage,
    SearchQuery,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{
    DownloadOutcome, DownloadRequest, Downloader, ExpectedHash, HashAlgorithm, ProgressFn,
};
use mcpanel_core::time::Timestamp;
use sha2::{Digest, Sha256, Sha512};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub const HOST: &str = "test.invalid";

pub fn plugin_jar(name: &str, version: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut buf);
    w.start_file("plugin.yml", zip::write::SimpleFileOptions::default())
        .unwrap();
    write!(w, "name: {name}\nversion: {version}\nmain: test.{name}\n").unwrap();
    w.finish().unwrap();
    buf.into_inner()
}

fn not_a_plugin() -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut buf);
    w.start_file("readme.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    w.write_all(b"hello").unwrap();
    w.finish().unwrap();
    buf.into_inner()
}

pub fn sha512(b: &[u8]) -> String {
    hex::encode(Sha512::digest(b))
}

#[derive(Default)]
pub struct Store {
    pub files: HashMap<String, Vec<u8>>,
    pub downloads: u32,
}

pub struct MemDownloader(pub Arc<Mutex<Store>>);

#[async_trait]
impl Downloader for MemDownloader {
    async fn download(
        &self,
        req: &DownloadRequest,
        dest: &Path,
        progress: &ProgressFn,
        _cancel: &CancellationToken,
    ) -> CoreResult<DownloadOutcome> {
        let bytes = {
            let mut s = self.0.lock().unwrap();
            s.downloads += 1;
            s.files
                .get(&req.url)
                .cloned()
                .ok_or_else(|| CoreError::new(ErrorCode::DownloadFailed, "404"))?
        };
        if let Some(h) = &req.expected_hash {
            let actual = match h.algorithm {
                HashAlgorithm::Sha512 => sha512(&bytes),
                HashAlgorithm::Sha256 => hex::encode(Sha256::digest(&bytes)),
                _ => return Err(CoreError::internal("unsupported in test")),
            };
            if actual != h.hex {
                return Err(CoreError::new(
                    ErrorCode::HashMismatch,
                    "The download does not match its hash",
                ));
            }
        }
        std::fs::write(dest, &bytes).map_err(|e| CoreError::io("write", &e))?;
        progress(bytes.len() as u64, Some(bytes.len() as u64));
        Ok(DownloadOutcome {
            bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
        })
    }
}

struct Ver {
    project: &'static str,
    id: &'static str,
    number: &'static str,
    file: Option<(&'static str, Vec<u8>, bool)>, // name, bytes, correct hash
    deps: Vec<&'static str>,
    published: i64,
}

pub struct TestHub {
    info: ContentProviderInfo,
    store: Arc<Mutex<Store>>,
    versions: Vec<Ver>,
}

impl TestHub {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        let v = |project,
                 id,
                 number,
                 file: Option<(&'static str, Vec<u8>, bool)>,
                 deps: Vec<&'static str>,
                 published| Ver {
            project,
            id,
            number,
            file,
            deps,
            published,
        };
        let versions = vec![
            v(
                "alpha",
                "a2",
                "2.0",
                Some(("Alpha-2.0.jar", plugin_jar("Alpha", "2.0"), true)),
                vec!["beta"],
                2000,
            ),
            v(
                "alpha",
                "a1",
                "1.0",
                Some(("Alpha-1.0.jar", plugin_jar("Alpha", "1.0"), true)),
                vec!["beta"],
                1000,
            ),
            v(
                "beta",
                "b1",
                "1.0",
                Some(("Beta-1.0.jar", plugin_jar("Beta", "1.0"), true)),
                vec![],
                1000,
            ),
            v(
                "notaplugin",
                "n1",
                "1.0",
                Some(("Readme-1.0.jar", not_a_plugin(), true)),
                vec![],
                1000,
            ),
            v(
                "badhash",
                "h1",
                "1.0",
                Some(("Bad-1.0.jar", plugin_jar("Bad", "1.0"), false)),
                vec![],
                1000,
            ),
            v("external", "e1", "1.0", None, vec![], 1000),
        ];
        {
            let mut s = store.lock().unwrap();
            for ver in &versions {
                if let Some((name, bytes, _)) = &ver.file {
                    s.files
                        .insert(format!("https://{HOST}/{name}"), bytes.clone());
                }
            }
        }
        Self {
            info: ContentProviderInfo {
                id: "testhub".into(),
                display_name: "TestHub".into(),
                website: format!("https://{HOST}"),
                download_hosts: vec![HOST.into()],
                kinds: vec![ContentKind::Plugin],
                hash_lookup: true,
            },
            store,
            versions,
        }
    }

    fn to_version(&self, v: &Ver) -> ContentVersion {
        ContentVersion {
            provider: "testhub".into(),
            project_id: v.project.into(),
            id: v.id.into(),
            name: v.number.into(),
            version_number: v.number.into(),
            channel: ReleaseChannel::Release,
            game_versions: vec!["1.21.4".into()],
            loaders: vec!["paper".into()],
            published: Some(Timestamp(v.published)),
            file: v.file.as_ref().map(|(name, bytes, ok)| ContentFile {
                url: format!("https://{HOST}/{name}"),
                file_name: name.to_string(),
                size: Some(bytes.len() as u64),
                hash: Some(ExpectedHash {
                    algorithm: HashAlgorithm::Sha512,
                    hex: if *ok {
                        sha512(bytes)
                    } else {
                        sha512(b"something else")
                    },
                }),
            }),
            external_url: v.file.is_none().then(|| format!("https://{HOST}/external")),
            dependencies: v
                .deps
                .iter()
                .map(|d| ContentDependency {
                    project_id: Some(d.to_string()),
                    version_id: None,
                    name: None,
                    kind: DependencyKind::Required,
                    external_url: None,
                })
                .collect(),
        }
    }

    pub fn downloads(&self) -> u32 {
        self.store.lock().unwrap().downloads
    }
}

#[async_trait]
impl ContentProvider for TestHub {
    fn info(&self) -> &ContentProviderInfo {
        &self.info
    }
    fn supports(&self, target: &ContentTarget) -> bool {
        target.kind == ContentKind::Plugin
    }
    async fn search(&self, q: &SearchQuery) -> CoreResult<SearchPage> {
        let mut hits = Vec::new();
        for p in ["alpha", "beta"] {
            if p.contains(&q.text.to_lowercase()) {
                hits.push(self.project(p).await?);
            }
        }
        Ok(SearchPage {
            total: hits.len() as u64,
            hits,
        })
    }
    async fn project(&self, id: &str) -> CoreResult<ProjectSummary> {
        if !self.versions.iter().any(|v| v.project == id) {
            return Err(CoreError::not_found("no such project"));
        }
        let mut name: String = id.to_string();
        name[..1].make_ascii_uppercase();
        Ok(ProjectSummary {
            provider: "testhub".into(),
            id: id.into(),
            slug: id.into(),
            name,
            description: "test".into(),
            author: None,
            downloads: 1,
            icon_url: None,
            page_url: format!("https://{HOST}/{id}"),
            updated: None,
            license: None,
        })
    }
    async fn versions(
        &self,
        project_id: &str,
        _t: &ContentTarget,
    ) -> CoreResult<Vec<ContentVersion>> {
        Ok(self
            .versions
            .iter()
            .filter(|v| v.project == project_id)
            .map(|v| self.to_version(v))
            .collect())
    }
    async fn version(&self, project_id: &str, version_id: &str) -> CoreResult<ContentVersion> {
        self.versions
            .iter()
            .find(|v| v.project == project_id && v.id == version_id)
            .map(|v| self.to_version(v))
            .ok_or_else(|| CoreError::not_found("no such version"))
    }
    async fn identify(&self, sha: &[String]) -> CoreResult<HashMap<String, ContentVersion>> {
        Ok(self
            .versions
            .iter()
            .filter_map(|v| {
                let (_, bytes, _) = v.file.as_ref()?;
                let h = sha512(bytes);
                sha.contains(&h).then(|| (h, self.to_version(v)))
            })
            .collect())
    }
}
