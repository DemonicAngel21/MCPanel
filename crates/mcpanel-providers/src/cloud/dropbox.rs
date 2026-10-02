//! Dropbox (API v2). Verified against Dropbox's documentation and developer forum answers:
//! - OAuth guide (docs.dropboxapi.com/dropbox-api/docs/oauth): PKCE with
//!   `code_challenge_method=S256` for apps that cannot keep a secret; the code exchange
//!   sends `code_verifier` instead of `client_secret`; `token_access_type=offline` returns
//!   a refresh token; refresh via `/oauth2/token` with `grant_type=refresh_token`.
//! - Redirect URIs must be pre-registered in the App Console and match exactly, including
//!   the port — Dropbox has no variable loopback port, so MCPanel uses three fixed ports
//!   (all three must be registered) and takes the first free one.
//! - Scoped apps: `account_info.read` is required for user-linked apps; files access via
//!   `files.metadata.read`, `files.content.read`, `files.content.write`. "App folder"
//!   access limits the app to its own folder.
//! - File upload:
//!   - Simple upload: `POST /2/files/upload` (for files up to 150 MB).
//!   - Chunked upload sessions: `/2/files/upload_session/start`,
//!     `/2/files/upload_session/append_v2`, and `/2/files/upload_session/finish` for large files.
//!   - Header `Dropbox-API-Arg`: JSON with path, mode, etc. All non-ASCII characters MUST
//!     be ASCII-escaped (`\uXXXX`) to be valid in HTTP headers.
//!   - Response errors: parsed safely and sanitized (user_message / error_summary / status).

use super::{Stage, api_json, token_request};
use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::cloud::{
    CloudAccount, CloudFileMetadata, CloudProviderInfo, CloudStorageProvider, CloudTransferOptions,
    RedirectSpec, RevokeOutcome, TokenSet,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::time::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::path::Path;

const NAME: &str = "Dropbox";
const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";
/// Loopback ports registered as redirect URIs (tried in order).
pub const PORTS: &[u16] = &[43917, 43918, 43919];

/// Simple upload threshold in bytes (8 MB). Files larger than this use chunked upload sessions
/// to provide continuous progress reporting and responsive cancellation.
pub const SIMPLE_UPLOAD_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;
/// Simple upload limit in bytes (150 MB, per Dropbox API documentation).
pub const SIMPLE_UPLOAD_MAX_BYTES: u64 = 150 * 1024 * 1024;
/// Chunk size for upload sessions (4 MB for responsive progress updates).
pub const CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// Format JSON into an ASCII-only string suitable for the `Dropbox-API-Arg` HTTP header.
/// Any character outside visible ASCII or control characters is encoded as `\uXXXX`.
pub(crate) fn format_dropbox_arg(val: &serde_json::Value) -> CoreResult<String> {
    let s = serde_json::to_string(val).map_err(|e| CoreError::internal(e.to_string()))?;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            out.push(c);
        } else {
            out.push_str(&format!("\\u{:04x}", c as u32));
        }
    }
    Ok(out)
}

/// Safely extract and sanitize an error message from a Dropbox API error response.
/// Never leaks secrets or raw authorization tokens.
pub(crate) fn sanitize_dropbox_error(status: u16, body_bytes: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body_bytes) {
        if let Some(user_msg) = v
            .get("user_message")
            .and_then(|m| m.get("text"))
            .and_then(|t| t.as_str())
        {
            return format!("HTTP {status}: {user_msg}");
        }
        if let Some(summary) = v.get("error_summary").and_then(|s| s.as_str()) {
            return format!("HTTP {status}: {summary}");
        }
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return format!("HTTP {status}: {err}");
        }
        if let Some(tag) = v
            .get("error")
            .and_then(|e| e.get(".tag"))
            .and_then(|t| t.as_str())
        {
            return format!("HTTP {status}: {tag}");
        }
    }
    let text = String::from_utf8_lossy(body_bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        format!("HTTP {status}")
    } else {
        let sanitized = trimmed.replace(['\r', '\n'], " ");
        let safe_text = if sanitized.len() > 300 {
            format!("{}...", &sanitized[..300])
        } else {
            sanitized
        };
        format!("HTTP {status}: {safe_text}")
    }
}

fn reqwest_error(action: &str, e: reqwest::Error) -> CoreError {
    let mut msg = format!("{NAME} {action} request failed: {e}");
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        msg.push_str(&format!(" (caused by: {s})"));
        src = std::error::Error::source(s);
    }
    CoreError::new(ErrorCode::ProviderError, msg)
}

pub struct Dropbox {
    http: HttpClient,
    info: CloudProviderInfo,
    content_base: String,
    api_base: String,
}

impl Dropbox {
    pub fn new(http: HttpClient, client_id: Option<String>) -> Self {
        Self::with_endpoints(
            http,
            client_id,
            "https://content.dropboxapi.com/2".to_string(),
            "https://api.dropboxapi.com/2".to_string(),
        )
    }

    pub fn with_endpoints(
        http: HttpClient,
        client_id: Option<String>,
        content_base: String,
        api_base: String,
    ) -> Self {
        Self {
            http,
            content_base,
            api_base,
            info: CloudProviderInfo {
                id: "dropbox",
                display_name: NAME,
                client_id,
                client_id_variable: super::DROPBOX_CLIENT_ID_VAR,
                authorize_url: "https://www.dropbox.com/oauth2/authorize",
                scopes: &[
                    "account_info.read",
                    "files.metadata.read",
                    "files.content.read",
                    "files.content.write",
                ],
                extra_authorize_params: &[("token_access_type", "offline")],
                redirect: RedirectSpec::FixedPorts {
                    host: "localhost",
                    path: "/mcpanel/oauth",
                    ports: PORTS,
                },
                manage_access_url: "https://www.dropbox.com/account/connected_apps",
            },
        }
    }

    #[cfg(test)]
    pub fn for_test(http: HttpClient, client_id: Option<String>, base_url: &str) -> Self {
        Self::with_endpoints(
            http,
            client_id,
            format!("{base_url}/content"),
            format!("{base_url}/api"),
        )
    }

    fn client_id(&self) -> CoreResult<&str> {
        self.info
            .client_id
            .as_deref()
            .ok_or_else(|| CoreError::new(ErrorCode::Unsupported, "Dropbox is not configured"))
    }

    fn parse_file_metadata(
        body_bytes: &[u8],
        fallback_name: &str,
        fallback_size: u64,
    ) -> CoreResult<CloudFileMetadata> {
        let parsed: serde_json::Value = serde_json::from_slice(body_bytes).map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} invalid file metadata response: {e}"),
            )
        })?;

        let id = parsed
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let name = parsed
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback_name)
            .to_string();
        let size_bytes = parsed
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(fallback_size);
        let modified_at = parsed
            .get("server_modified")
            .and_then(serde_json::Value::as_str)
            .and_then(Timestamp::parse_rfc3339);

        Ok(CloudFileMetadata {
            id,
            name,
            size_bytes,
            created_at: modified_at,
            modified_at,
            provider: "dropbox".to_string(),
        })
    }

    async fn send_dropbox_request(
        &self,
        access_token: &SecretString,
        url: &str,
        arg_header: &str,
        body: bytes::Bytes,
        action: &str,
        options: Option<&CloudTransferOptions>,
    ) -> CoreResult<bytes::Bytes> {
        let max_attempts = 4;
        let mut attempt = 0;
        loop {
            attempt += 1;

            if let Some(opts) = options
                && opts.cancel.is_cancelled()
            {
                return Err(CoreError::new(
                    ErrorCode::Cancelled,
                    "Upload cancelled by user",
                ));
            }

            let req = self
                .http
                .inner()
                .post(url)
                .bearer_auth(access_token.expose_secret())
                .header("Dropbox-API-Arg", arg_header)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .timeout(std::time::Duration::from_secs(60))
                .body(body.clone());

            let send_fut = req.send();
            let resp_res = if let Some(opts) = options {
                tokio::select! {
                    _ = opts.cancel.cancelled() => {
                        return Err(CoreError::new(ErrorCode::Cancelled, "Upload cancelled by user"));
                    }
                    res = send_fut => res,
                }
            } else {
                send_fut.await
            };

            match resp_res {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if (200..300).contains(&status) {
                        let bytes_fut = resp.bytes();
                        let bytes_res = if let Some(opts) = options {
                            tokio::select! {
                                _ = opts.cancel.cancelled() => {
                                    return Err(CoreError::new(ErrorCode::Cancelled, "Upload cancelled by user"));
                                }
                                res = bytes_fut => res,
                            }
                        } else {
                            bytes_fut.await
                        };
                        return bytes_res.map_err(|e| reqwest_error(&format!("{action} read"), e));
                    }

                    let retry_after = resp
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok());

                    let err_bytes = resp.bytes().await.unwrap_or_default();
                    let err_text = sanitize_dropbox_error(status, &err_bytes);

                    // Don't retry on 401 or non-transient errors
                    let is_transient = status == 429 || status >= 500;
                    if !is_transient || attempt >= max_attempts {
                        return Err(CoreError::new(
                            ErrorCode::ProviderError,
                            format!("{NAME} {action} failed: {err_text}"),
                        ));
                    }

                    let backoff_secs = if let Some(ra) = retry_after {
                        ra.min(10)
                    } else {
                        (1u64 << (attempt - 1)) / 2
                    };
                    let sleep_dur = std::time::Duration::from_millis(if backoff_secs == 0 {
                        500
                    } else {
                        backoff_secs * 1000
                    });

                    if let Some(opts) = options {
                        tokio::select! {
                            _ = opts.cancel.cancelled() => {
                                return Err(CoreError::new(ErrorCode::Cancelled, "Upload cancelled by user"));
                            }
                            _ = tokio::time::sleep(sleep_dur) => {}
                        }
                    } else {
                        tokio::time::sleep(sleep_dur).await;
                    }
                }
                Err(e) => {
                    if attempt >= max_attempts {
                        return Err(reqwest_error(action, e));
                    }
                    let sleep_dur = std::time::Duration::from_millis(500 * (1 << (attempt - 1)));
                    if let Some(opts) = options {
                        tokio::select! {
                            _ = opts.cancel.cancelled() => {
                                return Err(CoreError::new(ErrorCode::Cancelled, "Upload cancelled by user"));
                            }
                            _ = tokio::time::sleep(sleep_dur) => {}
                        }
                    } else {
                        tokio::time::sleep(sleep_dur).await;
                    }
                }
            }
        }
    }

    async fn upload_simple(
        &self,
        access_token: &SecretString,
        path: &Path,
        filename: &str,
        target_path: &str,
        file_size: u64,
        options: Option<&CloudTransferOptions>,
    ) -> CoreResult<CloudFileMetadata> {
        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| CoreError::io("Cannot read file to upload to Dropbox", &e))?;
        let arg = serde_json::json!({
            "path": target_path,
            "mode": "overwrite",
            "autorename": false,
            "mute": false,
            "strict_conflict": false
        });
        let arg_header = format_dropbox_arg(&arg)?;
        let url = format!("{}/files/upload", self.content_base);

        let body_bytes = self
            .send_dropbox_request(
                access_token,
                &url,
                &arg_header,
                bytes::Bytes::from(file_bytes),
                "upload",
                options,
            )
            .await?;

        if let Some(opts) = options {
            opts.report(file_size, file_size);
        }

        Self::parse_file_metadata(&body_bytes, filename, file_size)
    }

    #[allow(clippy::too_many_arguments)]
    async fn upload_chunked_session_impl(
        &self,
        access_token: &SecretString,
        path: &Path,
        filename: &str,
        target_path: &str,
        file_size: u64,
        chunk_size: usize,
        options: Option<&CloudTransferOptions>,
    ) -> CoreResult<CloudFileMetadata> {
        use tokio::io::AsyncReadExt;
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|e| CoreError::io("Cannot open file for upload session", &e))?;

        let mut buffer = vec![0u8; chunk_size];
        let n = file
            .read(&mut buffer)
            .await
            .map_err(|e| CoreError::io("Cannot read initial chunk for upload session", &e))?;

        // 1. Start upload session
        let start_url = format!("{}/files/upload_session/start", self.content_base);
        let start_arg = serde_json::json!({ "close": false });
        let start_header = format_dropbox_arg(&start_arg)?;

        let start_body = self
            .send_dropbox_request(
                access_token,
                &start_url,
                &start_header,
                bytes::Bytes::copy_from_slice(&buffer[..n]),
                "upload session start",
                options,
            )
            .await?;

        let start_resp: serde_json::Value = serde_json::from_slice(&start_body).map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} invalid upload session start response: {e}"),
            )
        })?;
        let session_id = start_resp
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    format!("{NAME} did not return session_id for upload session"),
                )
            })?
            .to_string();

        let mut uploaded_offset = n as u64;
        if let Some(opts) = options {
            opts.report(uploaded_offset, file_size);
        }

        // 2. Append chunks until the final chunk
        while uploaded_offset + (chunk_size as u64) < file_size {
            let n = file
                .read(&mut buffer)
                .await
                .map_err(|e| CoreError::io("Cannot read chunk for upload session", &e))?;
            if n == 0 {
                break;
            }

            let append_url = format!("{}/files/upload_session/append_v2", self.content_base);
            let append_arg = serde_json::json!({
                "cursor": {
                    "session_id": session_id,
                    "offset": uploaded_offset
                },
                "close": false
            });
            let append_header = format_dropbox_arg(&append_arg)?;

            self.send_dropbox_request(
                access_token,
                &append_url,
                &append_header,
                bytes::Bytes::copy_from_slice(&buffer[..n]),
                "upload session append",
                options,
            )
            .await?;

            uploaded_offset += n as u64;
            if let Some(opts) = options {
                opts.report(uploaded_offset, file_size);
            }
        }

        // 3. Final chunk and finish
        let mut final_chunk = Vec::new();
        file.read_to_end(&mut final_chunk)
            .await
            .map_err(|e| CoreError::io("Cannot read final chunk for upload session", &e))?;

        let finish_url = format!("{}/files/upload_session/finish", self.content_base);
        let finish_arg = serde_json::json!({
            "cursor": {
                "session_id": session_id,
                "offset": uploaded_offset
            },
            "commit": {
                "path": target_path,
                "mode": "overwrite",
                "autorename": false,
                "mute": false,
                "strict_conflict": false
            }
        });
        let finish_header = format_dropbox_arg(&finish_arg)?;

        let finish_body = self
            .send_dropbox_request(
                access_token,
                &finish_url,
                &finish_header,
                bytes::Bytes::from(final_chunk),
                "upload session finish",
                options,
            )
            .await?;

        if let Some(opts) = options {
            opts.report(file_size, file_size);
        }

        Self::parse_file_metadata(&finish_body, filename, file_size)
    }
}

#[derive(Deserialize)]
struct Account {
    name: Option<Name>,
    email: Option<String>,
}

#[derive(Deserialize)]
struct Name {
    display_name: Option<String>,
}

#[async_trait]
impl CloudStorageProvider for Dropbox {
    fn info(&self) -> &CloudProviderInfo {
        &self.info
    }

    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> CoreResult<TokenSet> {
        token_request(
            &self.http,
            NAME,
            Stage::Exchange,
            TOKEN_URL,
            &[
                ("code", code),
                ("grant_type", "authorization_code"),
                ("code_verifier", verifier),
                ("client_id", self.client_id()?),
                ("redirect_uri", redirect_uri),
            ],
        )
        .await
    }

    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<TokenSet> {
        token_request(
            &self.http,
            NAME,
            Stage::Refresh,
            TOKEN_URL,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.expose_secret()),
                ("client_id", self.client_id()?),
            ],
        )
        .await
    }

    async fn account(&self, access_token: &SecretString) -> CoreResult<CloudAccount> {
        let url = format!("{}/users/get_current_account", self.api_base);
        let (status, body) = self
            .http
            .send_raw(
                reqwest::Method::POST,
                &url,
                Some(access_token.expose_secret()),
                None,
            )
            .await?;
        let a: Account = api_json(NAME, "account lookup", status, &body)?;
        Ok(CloudAccount {
            display_name: a.name.and_then(|n| n.display_name),
            email: a.email,
        })
    }

    async fn revoke(&self, refresh_token: &SecretString) -> CoreResult<RevokeOutcome> {
        // Revoking needs an access token; a refresh gives one for this grant.
        let tokens = match self.refresh(refresh_token).await {
            Ok(t) => t,
            // Already invalid: nothing to revoke.
            Err(_) => return Ok(RevokeOutcome::Revoked),
        };
        let url = format!("{}/auth/token/revoke", self.api_base);
        let (status, _) = self
            .http
            .send_raw(
                reqwest::Method::POST,
                &url,
                Some(tokens.access_token.expose_secret()),
                None,
            )
            .await?;
        if (200..300).contains(&status) {
            Ok(RevokeOutcome::Revoked)
        } else {
            Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME}: revocation failed (HTTP {status})"),
            ))
        }
    }

    async fn upload_file(
        &self,
        access_token: &SecretString,
        filename: &str,
        path: &Path,
        options: Option<CloudTransferOptions>,
    ) -> CoreResult<CloudFileMetadata> {
        let clean = filename.trim().trim_matches(|c| c == '/' || c == '\\');
        if clean.is_empty()
            || clean == "."
            || clean == ".."
            || clean.contains('/')
            || clean.contains('\\')
        {
            return Err(CoreError::new(
                ErrorCode::InvalidInput,
                "Invalid backup filename for cloud upload",
            ));
        }
        let target_path = format!("/backups/{clean}");
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| CoreError::io("Cannot access backup file to upload", &e))?;
        let file_size = meta.len();

        if file_size <= SIMPLE_UPLOAD_THRESHOLD_BYTES {
            self.upload_simple(
                access_token,
                path,
                clean,
                &target_path,
                file_size,
                options.as_ref(),
            )
            .await
        } else {
            self.upload_chunked_session_impl(
                access_token,
                path,
                clean,
                &target_path,
                file_size,
                CHUNK_SIZE,
                options.as_ref(),
            )
            .await
        }
    }

    async fn list_files(&self, access_token: &SecretString) -> CoreResult<Vec<CloudFileMetadata>> {
        let body = serde_json::json!({
            "path": "/backups",
            "recursive": false
        });
        let url = format!("{}/files/list_folder", self.api_base);

        let resp = self
            .http
            .inner()
            .post(&url)
            .bearer_auth(access_token.expose_secret())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| reqwest_error("list", e))?;

        let status = resp.status().as_u16();
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| reqwest_error("list read", e))?;

        if status == 409 {
            let err_text = String::from_utf8_lossy(&body_bytes);
            if err_text.contains("path/not_found") || err_text.contains("not_found") {
                return Ok(vec![]);
            }
        }

        if !(200..300).contains(&status) {
            let err_text = sanitize_dropbox_error(status, &body_bytes);
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} list failed: {err_text}"),
            ));
        }

        let parsed: serde_json::Value =
            serde_json::from_slice(&body_bytes).map_err(|e| CoreError::internal(e.to_string()))?;

        let mut out = Vec::new();
        if let Some(entries) = parsed.get("entries").and_then(serde_json::Value::as_array) {
            for e in entries {
                if e.get(".tag").and_then(serde_json::Value::as_str) != Some("file") {
                    continue;
                }
                let name = e
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if !name.ends_with(".zip") && !name.ends_with(".zip.age") {
                    continue;
                }
                let id = e
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let size_bytes = e
                    .get("size")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let modified_at = e
                    .get("server_modified")
                    .and_then(serde_json::Value::as_str)
                    .and_then(Timestamp::parse_rfc3339);

                out.push(CloudFileMetadata {
                    id,
                    name,
                    size_bytes,
                    created_at: modified_at,
                    modified_at,
                    provider: "dropbox".to_string(),
                });
            }
        }
        Ok(out)
    }

    async fn download_file(
        &self,
        access_token: &SecretString,
        file_id: &str,
        destination: &Path,
        options: Option<CloudTransferOptions>,
    ) -> CoreResult<u64> {
        use futures::StreamExt;
        if let Some(opts) = &options
            && opts.cancel.is_cancelled()
        {
            return Err(CoreError::new(
                ErrorCode::Cancelled,
                "Download cancelled by user",
            ));
        }

        let path_arg = if file_id.starts_with("id:") || file_id.starts_with('/') {
            file_id.to_string()
        } else {
            format!("/backups/{file_id}")
        };
        let arg = serde_json::json!({ "path": path_arg });
        let arg_header = format_dropbox_arg(&arg)?;
        let url = format!("{}/files/download", self.content_base);

        let req = self
            .http
            .inner()
            .post(&url)
            .bearer_auth(access_token.expose_secret())
            .header("Dropbox-API-Arg", arg_header)
            .send();

        let resp = if let Some(opts) = &options {
            tokio::select! {
                _ = opts.cancel.cancelled() => {
                    return Err(CoreError::new(ErrorCode::Cancelled, "Download cancelled by user"));
                }
                res = req => res.map_err(|e| reqwest_error("download", e))?,
            }
        } else {
            req.await.map_err(|e| reqwest_error("download", e))?
        };

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body_bytes = resp.bytes().await.unwrap_or_default();
            let err_text = sanitize_dropbox_error(status, &body_bytes);
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} download failed: {err_text}"),
            ));
        }

        let expected_total = resp.content_length();

        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CoreError::io("Cannot create destination directory", &e))?;
        }

        let partial_name = match destination.file_name() {
            Some(f) => format!("{}.partial", f.to_string_lossy()),
            None => "download.partial".to_string(),
        };
        let partial = destination.with_file_name(partial_name);
        let mut file = match tokio::fs::File::create(&partial).await {
            Ok(f) => f,
            Err(e) => return Err(CoreError::io("Cannot create download target file", &e)),
        };

        let mut stream = resp.bytes_stream();
        let mut total_bytes = 0u64;

        loop {
            let chunk_opt = if let Some(opts) = &options {
                tokio::select! {
                    _ = opts.cancel.cancelled() => {
                        drop(file);
                        let _ = tokio::fs::remove_file(&partial).await;
                        return Err(CoreError::new(ErrorCode::Cancelled, "Download cancelled by user"));
                    }
                    next = stream.next() => next,
                }
            } else {
                stream.next().await
            };

            match chunk_opt {
                Some(Ok(data)) => {
                    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &data).await {
                        drop(file);
                        let _ = tokio::fs::remove_file(&partial).await;
                        return Err(CoreError::io("Cannot write to download file", &e));
                    }
                    total_bytes += data.len() as u64;
                    if let Some(opts) = &options {
                        opts.report(total_bytes, expected_total.unwrap_or(total_bytes));
                    }
                }
                Some(Err(e)) => {
                    drop(file);
                    let _ = tokio::fs::remove_file(&partial).await;
                    return Err(CoreError::new(
                        ErrorCode::ProviderError,
                        format!("{NAME} download stream error: {e}"),
                    ));
                }
                None => break,
            }
        }

        if let Err(e) = tokio::io::AsyncWriteExt::flush(&mut file).await {
            drop(file);
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(CoreError::io("Cannot flush download file", &e));
        }
        drop(file);

        if let Err(e) = tokio::fs::rename(&partial, destination).await {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(CoreError::io("Cannot finish download target file", &e));
        }

        Ok(total_bytes)
    }

    async fn delete_file(&self, access_token: &SecretString, file_id: &str) -> CoreResult<()> {
        let path_arg = if file_id.starts_with("id:") || file_id.starts_with('/') {
            file_id.to_string()
        } else {
            format!("/backups/{file_id}")
        };
        let body = serde_json::json!({ "path": path_arg });
        let url = format!("{}/files/delete_v2", self.api_base);

        let resp = self
            .http
            .inner()
            .post(&url)
            .bearer_auth(access_token.expose_secret())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| reqwest_error("delete", e))?;

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) && status != 404 && status != 409 {
            let body_bytes = resp.bytes().await.unwrap_or_default();
            let err_text = sanitize_dropbox_error(status, &body_bytes);
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} delete failed: {err_text}"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn format_dropbox_arg_ascii_and_unicode() {
        let normal = serde_json::json!({
            "path": "/backups/normal_backup.zip",
            "mode": "overwrite"
        });
        let arg = format_dropbox_arg(&normal).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&arg).unwrap();
        assert_eq!(parsed, normal);
        assert!(arg.is_ascii());

        let unicode = serde_json::json!({
            "path": "/backups/münchen_café.zip"
        });
        let arg_uni = format_dropbox_arg(&unicode).unwrap();
        assert!(arg_uni.is_ascii(), "Must be pure ASCII: {arg_uni}");
        assert!(arg_uni.contains(r#"\u00fc"#), "Umlaut encoded");
        assert!(arg_uni.contains(r#"\u00e9"#), "Accent encoded");
    }

    #[test]
    fn sanitize_dropbox_error_formats() {
        let err_summary = br#"{"error_summary":"path/conflict/file/..","error":{".tag":"path"}}"#;
        assert_eq!(
            sanitize_dropbox_error(409, err_summary),
            "HTTP 409: path/conflict/file/.."
        );

        let user_msg = br#"{"user_message":{"text":"Your Dropbox is out of space."}}"#;
        assert_eq!(
            sanitize_dropbox_error(507, user_msg),
            "HTTP 507: Your Dropbox is out of space."
        );

        let tagged = br#"{"error":{".tag":"expired_access_token"}}"#;
        assert_eq!(
            sanitize_dropbox_error(401, tagged),
            "HTTP 401: expired_access_token"
        );

        let raw = b"502 Bad Gateway\nnginx";
        assert_eq!(
            sanitize_dropbox_error(502, raw),
            "HTTP 502: 502 Bad Gateway nginx"
        );
    }

    type RequestLog = std::sync::Arc<std::sync::Mutex<Vec<(String, String, Vec<u8>)>>>;

    async fn mock_server(replies: Vec<(u16, &'static str)>) -> (String, RequestLog) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let log: RequestLog = std::sync::Arc::default();
        let log_clone = std::sync::Arc::clone(&log);

        tokio::spawn(async move {
            for (status, body) in replies {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        let len: usize = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                            let req_body = buf[end + 4..end + 4 + len].to_vec();
                            log_clone.lock().unwrap().push((path, head, req_body));
                            break;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });

        (base, log)
    }

    #[tokio::test]
    async fn successful_simple_upload() {
        let (base, log) = mock_server(vec![(
            200,
            r#"{"id":"id:file123","name":"backup.zip","size":12,"server_modified":"2026-10-02T12:00:00Z"}"#,
        )])
        .await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("backup.zip");
        tokio::fs::write(&backup_path, b"hello backup")
            .await
            .unwrap();

        let token = SecretString::from("token-123".to_string());
        let meta = d
            .upload_file(&token, "backup.zip", &backup_path, None)
            .await
            .unwrap();

        assert_eq!(meta.id, "id:file123");
        assert_eq!(meta.name, "backup.zip");
        assert_eq!(meta.size_bytes, 12);
        assert_eq!(meta.provider, "dropbox");

        let log = log.lock().unwrap().clone();
        assert_eq!(log.len(), 1);
        let (path, head, body) = &log[0];
        assert_eq!(path, "/content/files/upload");
        assert!(
            head.contains("authorization: Bearer token-123") || head.contains("Bearer token-123")
        );
        assert!(head.contains("dropbox-api-arg:"));
        assert!(head.contains("/backups/backup.zip"));
        assert_eq!(body, b"hello backup");
    }

    #[tokio::test]
    async fn successful_chunked_session_upload() {
        let (base, log) = mock_server(vec![
            // 1. start
            (200, r#"{"session_id":"session-xyz-42"}"#),
            // 2. append_v2
            (200, r#"{}"#),
            // 3. finish
            (
                200,
                r#"{"id":"id:chunked123","name":"large.zip","size":2500,"server_modified":"2026-10-02T12:30:00Z"}"#,
            ),
        ])
        .await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("large.zip");
        let test_data = vec![0x42u8; 2500];
        tokio::fs::write(&backup_path, &test_data).await.unwrap();

        let token = SecretString::from("token-chunked".to_string());
        let meta = d
            .upload_chunked_session_impl(
                &token,
                &backup_path,
                "large.zip",
                "/backups/large.zip",
                2500,
                1000, // 1000 byte chunk size: chunks of 1000, 1000, 500
                None,
            )
            .await
            .unwrap();

        assert_eq!(meta.id, "id:chunked123");
        assert_eq!(meta.size_bytes, 2500);

        let requests = log.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].0, "/content/files/upload_session/start");
        assert_eq!(requests[0].2.len(), 1000);
        assert_eq!(requests[1].0, "/content/files/upload_session/append_v2");
        assert_eq!(requests[1].2.len(), 1000);
        assert!(requests[1].1.contains("session-xyz-42"));
        assert_eq!(requests[2].0, "/content/files/upload_session/finish");
        assert_eq!(requests[2].2.len(), 500);
    }

    #[tokio::test]
    async fn upload_http_error_response() {
        let (base, _) = mock_server(vec![(
            409,
            r#"{"error_summary":"path/conflict/file/..","error":{".tag":"path"}}"#,
        )])
        .await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("backup.zip");
        tokio::fs::write(&backup_path, b"test").await.unwrap();

        let token = SecretString::from("token-err".to_string());
        let err = d
            .upload_file(&token, "backup.zip", &backup_path, None)
            .await
            .unwrap_err();

        assert!(
            err.message.contains("HTTP 409: path/conflict/file/.."),
            "Error was: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn upload_malformed_response() {
        let (base, _) = mock_server(vec![(200, "not-json-garbage")]).await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("backup.zip");
        tokio::fs::write(&backup_path, b"test").await.unwrap();

        let token = SecretString::from("token-err".to_string());
        let err = d
            .upload_file(&token, "backup.zip", &backup_path, None)
            .await
            .unwrap_err();

        assert!(
            err.message.contains("invalid file metadata response"),
            "Error was: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn invalid_path_validation() {
        let d = Dropbox::new(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
        );
        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("backup.zip");
        tokio::fs::write(&backup_path, b"test").await.unwrap();

        let token = SecretString::from("token".to_string());
        let err1 = d
            .upload_file(&token, "", &backup_path, None)
            .await
            .unwrap_err();
        assert_eq!(err1.code, ErrorCode::InvalidInput);

        let err2 = d
            .upload_file(&token, "../..", &backup_path, None)
            .await
            .unwrap_err();
        assert_eq!(err2.code, ErrorCode::InvalidInput);

        let non_existent = temp_dir.path().join("does_not_exist.zip");
        let err3 = d
            .upload_file(&token, "backup.zip", &non_existent, None)
            .await
            .unwrap_err();
        assert_eq!(err3.code, ErrorCode::PathNotFound);
    }

    #[tokio::test]
    async fn upload_failure_preserves_local_backup() {
        let (base, _) = mock_server(vec![(500, r#"{"error_summary":"internal_error"}"#)]).await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("preserved_backup.zip");
        let original_content = b"CRITICAL LOCAL BACKUP DATA DO NOT CORRUPT";
        tokio::fs::write(&backup_path, original_content)
            .await
            .unwrap();

        let token = SecretString::from("token-err".to_string());
        let res = d
            .upload_file(&token, "preserved_backup.zip", &backup_path, None)
            .await;
        assert!(res.is_err());

        // Assert local file still exists and has exact same contents
        assert!(backup_path.exists());
        let disk_content = tokio::fs::read(&backup_path).await.unwrap();
        assert_eq!(disk_content, original_content);
    }

    #[tokio::test]
    async fn upload_cancellation_aborts_and_preserves_local_file() {
        let (base, _) = mock_server(vec![(200, r#"{"session_id":"session-xyz"}"#)]).await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("cancel_test.zip");
        let original_data = vec![0xABu8; 1000];
        tokio::fs::write(&backup_path, &original_data)
            .await
            .unwrap();

        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel(); // Pre-cancelled

        let options = CloudTransferOptions {
            on_progress: None,
            cancel,
        };

        let token = SecretString::from("token-cancel".to_string());
        let err = d
            .upload_file(&token, "cancel_test.zip", &backup_path, Some(options))
            .await
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::Cancelled);
        assert!(backup_path.exists());
        let on_disk = tokio::fs::read(&backup_path).await.unwrap();
        assert_eq!(on_disk, original_data);
    }

    #[tokio::test]
    async fn download_cancellation_cleans_partial_file() {
        let (base, _) = mock_server(vec![(200, "some large remote backup content here")]).await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let dest = temp_dir.path().join("downloaded.zip");

        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel(); // Pre-cancelled

        let options = CloudTransferOptions {
            on_progress: None,
            cancel,
        };

        let token = SecretString::from("token-cancel".to_string());
        let err = d
            .download_file(&token, "file123", &dest, Some(options))
            .await
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::Cancelled);
        assert!(!dest.exists());
        let partial = dest.with_file_name("downloaded.zip.partial");
        assert!(!partial.exists());
    }

    #[tokio::test]
    async fn upload_progress_updates_reported() {
        let (base, _) = mock_server(vec![
            (200, r#"{"session_id":"sess-123"}"#),
            (200, r#"{}"#),
            (
                200,
                r#"{"id":"id:fin","name":"progress.zip","size":3000,"server_modified":"2026-10-02T12:00:00Z"}"#,
            ),
        ])
        .await;

        let d = Dropbox::for_test(
            crate::http::HttpClient::for_test().unwrap(),
            Some("test-client".into()),
            &base,
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let backup_path = temp_dir.path().join("progress.zip");
        let data = vec![0x11u8; 3000];
        tokio::fs::write(&backup_path, &data).await.unwrap();

        let progress_records = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let pr_clone = std::sync::Arc::clone(&progress_records);
        let on_progress: mcpanel_core::cloud::CloudProgressFn =
            std::sync::Arc::new(move |completed, total| {
                pr_clone.lock().unwrap().push((completed, total));
            });

        let options = CloudTransferOptions {
            on_progress: Some(on_progress),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let token = SecretString::from("token".to_string());
        let meta = d
            .upload_chunked_session_impl(
                &token,
                &backup_path,
                "progress.zip",
                "/backups/progress.zip",
                3000,
                1000,
                Some(&options),
            )
            .await
            .unwrap();

        assert_eq!(meta.id, "id:fin");
        let records = progress_records.lock().unwrap().clone();
        assert!(!records.is_empty());
        // Verify final record reached 3000/3000
        assert_eq!(records.last().unwrap(), &(3000, Some(3000)));
    }
}
