//! Shared HTTPS client and the verified downloader.

use async_trait::async_trait;
use futures::StreamExt;
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{
    DownloadOutcome, DownloadRequest, Downloader, HashAlgorithm, ProgressFn,
};
use serde::de::DeserializeOwned;
use sha1::Digest;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

pub fn user_agent() -> String {
    format!(
        "MCPanel/{} (+https://github.com/mcpanel/mcpanel)",
        env!("CARGO_PKG_VERSION")
    )
}

#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
}

fn net_err(context: &str, e: reqwest::Error) -> CoreError {
    let retryable = e.is_timeout()
        || e.is_connect()
        || e.status()
            .is_some_and(|s| s.is_server_error() || s.as_u16() == 429);
    let err = CoreError::new(ErrorCode::ProviderUnavailable, format!("{context}: {e}"));
    if retryable { err.retryable() } else { err }
}

/// Retry transient failures (connection errors, timeouts, 5xx, 429) of metadata requests:
/// up to 3 attempts, waiting 1 s and 3 s. Anything else fails immediately.
async fn with_retries<T, F, Fut>(mut f: F) -> CoreResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = CoreResult<T>>,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Err(e) if e.retryable && attempt < 2 => {
                attempt += 1;
                tracing::warn!(target: "mcpanel::api", attempt, "provider request failed, retrying: {}", e.message);
                tokio::time::sleep(Duration::from_secs(if attempt == 1 { 1 } else { 3 })).await;
            }
            r => return r,
        }
    }
}

impl HttpClient {
    pub fn new() -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent())
            .https_only(true)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| CoreError::internal(format!("Cannot create HTTP client: {e}")))?;
        Ok(Self { client })
    }

    /// GET a JSON document (metadata-sized, bounded to 16 MiB).
    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> CoreResult<T> {
        with_retries(|| self.get_json_once(url)).await
    }

    async fn get_json_once<T: DeserializeOwned>(&self, url: &str) -> CoreResult<T> {
        let resp = self
            .client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| net_err("Request failed", e))?;
        read_json(resp).await
    }

    /// POST a JSON body and read a JSON response (same limits as [`Self::get_json`]).
    pub async fn post_json<B: serde::Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> CoreResult<T> {
        let resp = self
            .client
            .post(url)
            .json(body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| net_err("Request failed", e))?;
        read_json(resp).await
    }

    pub async fn get_bytes(&self, url: &str, max: usize) -> CoreResult<Vec<u8>> {
        with_retries(|| self.get_bytes_once(url, max)).await
    }

    async fn get_bytes_once(&self, url: &str, max: usize) -> CoreResult<Vec<u8>> {
        let resp = self
            .client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| net_err("Request failed", e))?
            .error_for_status()
            .map_err(|e| net_err("Request failed", e))?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| net_err("Reading response failed", e))?;
        if bytes.len() > max {
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                "Provider response is too large",
            ));
        }
        Ok(bytes.to_vec())
    }

    pub fn inner(&self) -> &reqwest::Client {
        &self.client
    }
}

enum Hasher {
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
    Md5(md5::Md5),
}

impl Hasher {
    fn new(a: HashAlgorithm) -> Self {
        match a {
            HashAlgorithm::Sha1 => Self::Sha1(sha1::Sha1::new()),
            HashAlgorithm::Sha256 => Self::Sha256(sha2::Sha256::new()),
            HashAlgorithm::Sha512 => Self::Sha512(sha2::Sha512::new()),
            HashAlgorithm::Md5 => Self::Md5(md5::Md5::new()),
        }
    }
    fn update(&mut self, d: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(d),
            Self::Sha256(h) => h.update(d),
            Self::Sha512(h) => h.update(d),
            Self::Md5(h) => h.update(d),
        }
    }
    fn finish(self) -> String {
        match self {
            Self::Sha1(h) => hex::encode(h.finalize()),
            Self::Sha256(h) => hex::encode(h.finalize()),
            Self::Sha512(h) => hex::encode(h.finalize()),
            Self::Md5(h) => hex::encode(h.finalize()),
        }
    }
}

pub struct HttpDownloader {
    http: HttpClient,
}

impl HttpDownloader {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }

    async fn run(
        &self,
        req: &DownloadRequest,
        dest: &Path,
        progress: &ProgressFn,
        cancel: &CancellationToken,
    ) -> CoreResult<DownloadOutcome> {
        if !req.url.starts_with("https://") {
            return Err(CoreError::new(
                ErrorCode::DownloadFailed,
                "Only HTTPS downloads are allowed",
            ));
        }
        let resp = self
            .http
            .inner()
            .get(&req.url)
            .send()
            .await
            .map_err(|e| net_err("Download failed", e))?
            .error_for_status()
            .map_err(|e| net_err("Download failed", e))?;
        let total = resp.content_length().or(req.expected_size);
        if let (Some(len), Some(exp)) = (resp.content_length(), req.expected_size)
            && len != exp
        {
            return Err(CoreError::new(
                ErrorCode::DownloadFailed,
                format!("Download size mismatch (expected {exp} bytes, server reports {len})"),
            ));
        }
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| CoreError::io("Cannot create download file", &e))?;
        let mut sha256 = sha2::Sha256::new();
        let mut expected = req.expected_hash.as_ref().map(|h| Hasher::new(h.algorithm));
        let mut done: u64 = 0;
        let limit = req
            .expected_size
            .map(|s| s + 1)
            .unwrap_or(4 * 1024 * 1024 * 1024);
        let mut stream = resp.bytes_stream();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(CoreError::cancelled()),
                c = stream.next() => c,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| net_err("Download interrupted", e))?;
            done += chunk.len() as u64;
            if done > limit {
                return Err(CoreError::new(
                    ErrorCode::DownloadFailed,
                    "Download is larger than expected",
                ));
            }
            sha256.update(&chunk);
            if let Some(h) = expected.as_mut() {
                h.update(&chunk);
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| CoreError::io("Cannot write download", &e))?;
            progress(done, total);
        }
        file.flush()
            .await
            .map_err(|e| CoreError::io("Cannot write download", &e))?;
        file.sync_all()
            .await
            .map_err(|e| CoreError::io("Cannot write download", &e))?;
        drop(file);
        if let Some(exp) = req.expected_size
            && exp != done
        {
            return Err(CoreError::new(
                ErrorCode::DownloadFailed,
                format!("Incomplete download ({done} of {exp} bytes)"),
            ));
        }
        if let (Some(h), Some(exp)) = (expected, &req.expected_hash) {
            let actual = h.finish();
            if !actual.eq_ignore_ascii_case(&exp.hex) {
                return Err(CoreError::new(
                    ErrorCode::HashMismatch,
                    "The downloaded file failed integrity verification and was discarded",
                ));
            }
        }
        Ok(DownloadOutcome {
            bytes: done,
            sha256: hex::encode(sha256.finalize()),
        })
    }
}

#[async_trait]
impl Downloader for HttpDownloader {
    async fn download(
        &self,
        req: &DownloadRequest,
        dest: &Path,
        progress: &ProgressFn,
        cancel: &CancellationToken,
    ) -> CoreResult<DownloadOutcome> {
        let result = self.run(req, dest, progress, cancel).await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(dest).await;
        }
        result
    }
}

/// Parse an RFC 3339 timestamp (`2026-09-15T16:53:02+00:00`, `…Z`, fractional seconds)
/// into unix milliseconds.
async fn read_json<T: DeserializeOwned>(resp: reqwest::Response) -> CoreResult<T> {
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err(CoreError::new(
            ErrorCode::VersionNotFound,
            "The requested version was not found",
        ));
    }
    if !status.is_success() {
        let err = CoreError::new(
            ErrorCode::ProviderError,
            format!("Provider returned HTTP {status}"),
        );
        return Err(if status.is_server_error() || status.as_u16() == 429 {
            err.retryable()
        } else {
            err
        });
    }
    if resp.content_length().is_some_and(|l| l > 16 * 1024 * 1024) {
        return Err(CoreError::new(
            ErrorCode::ProviderError,
            "Provider response is too large",
        ));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| net_err("Reading response failed", e))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(CoreError::new(
            ErrorCode::ProviderError,
            "Provider response is too large",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        CoreError::new(
            ErrorCode::ProviderError,
            format!("Unexpected provider response: {e}"),
        )
    })
}

pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut i = 19;
    let mut millis = 0i64;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let frac = &s[start..i];
        let padded = format!("{:0<3}", &frac[..frac.len().min(3)]);
        millis = padded.parse().ok()?;
    }
    let offset_secs = match b.get(i) {
        Some(b'Z') | Some(b'z') | None => 0,
        Some(sign @ (b'+' | b'-')) => {
            let oh: i64 = s.get(i + 1..i + 3)?.parse().ok()?;
            let om: i64 = s.get(i + 4..i + 6)?.parse().ok()?;
            let o = oh * 3600 + om * 60;
            if *sign == b'+' { o } else { -o }
        }
        _ => return None,
    };
    // Days from civil (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + h * 3600 + mi * 60 + se - offset_secs) * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339("2026-09-15T16:53:02+00:00"),
            Some(1_789_491_182_000)
        );
        assert_eq!(
            parse_rfc3339("2026-09-15T18:53:02+02:00"),
            Some(1_789_491_182_000)
        );
        assert_eq!(
            parse_rfc3339("2026-05-11T11:43:09.5Z"),
            Some(1_778_499_789_500)
        );
        assert_eq!(parse_rfc3339("nope"), None);
    }
}
