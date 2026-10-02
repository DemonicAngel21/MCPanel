//! Google Drive (Drive API v3). Verified against Google's documentation:
//! - OAuth 2.0 for installed apps (developers.google.com/identity/protocols/oauth2/native-app):
//!   client type "Desktop app"; loopback redirect `http://127.0.0.1:{port}/` on any free
//!   port; PKCE S256; authorization endpoint `https://accounts.google.com/o/oauth2/v2/auth`,
//!   token endpoint `https://oauth2.googleapis.com/token`.
//! - As a public client, MCPanel desktop does not require a confidential client secret and
//!   authenticates using Authorization Code + PKCE (`code_verifier`).
//!   Note: The OAuth client ID in Google Cloud Console MUST be created with application type
//!   "Desktop app". If an OAuth client ID is configured as "Web application" (e.g. Firebase Web),
//!   Google's token server will reject the code exchange with "client_secret is missing".
//! - Scopes: `https://www.googleapis.com/auth/drive.file` (non-sensitive): files the app
//!   creates or the user opens with it. `about.get` accepts it (`fields` is required).

use super::{Stage, api_json, token_request};
use crate::http::HttpClient;
use async_trait::async_trait;
use mcpanel_core::cloud::service::encode_form;
use mcpanel_core::cloud::{
    CloudAccount, CloudFileMetadata, CloudProviderInfo, CloudStorageProvider, CloudTransferOptions,
    RedirectSpec, RevokeOutcome, TokenSet,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::time::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::path::Path;

const NAME: &str = "Google Drive";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub const RESUMABLE_UPLOAD_THRESHOLD_BYTES: u64 = 5 * 1024 * 1024;
pub const DRIVE_CHUNK_SIZE: usize = 4 * 1024 * 1024;

pub struct GoogleDrive {
    http: HttpClient,
    info: CloudProviderInfo,
    /// Desktop client secret (non-confidential, see the module docs). Never logged.
    client_secret: Option<SecretString>,
}

impl GoogleDrive {
    pub fn new(http: HttpClient, client_id: Option<String>) -> Self {
        Self::with_secret(http, client_id, None)
    }

    pub fn with_secret(
        http: HttpClient,
        client_id: Option<String>,
        client_secret: Option<SecretString>,
    ) -> Self {
        Self {
            http,
            client_secret,
            info: CloudProviderInfo {
                id: "google_drive",
                display_name: NAME,
                client_id,
                client_id_variable: super::GOOGLE_CLIENT_ID_VAR,
                authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
                scopes: &["https://www.googleapis.com/auth/drive.file"],
                extra_authorize_params: &[
                    ("access_type", "offline"),
                    ("prompt", "consent select_account"),
                ],
                redirect: RedirectSpec::AnyPort {
                    host: "127.0.0.1",
                    path: "/",
                },
                manage_access_url: "https://myaccount.google.com/connections",
            },
        }
    }

    /// Form fields of the authorization-code exchange (PKCE verifier + client secret).
    fn exchange_params<'a>(
        &'a self,
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
    ) -> CoreResult<Vec<(&'a str, &'a str)>> {
        let mut p = vec![
            ("client_id", self.client_id()?),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ];
        if let Some(s) = &self.client_secret {
            p.push(("client_secret", s.expose_secret()));
        }
        Ok(p)
    }

    /// Form fields of the refresh-token exchange (client secret included).
    fn refresh_params<'a>(
        &'a self,
        refresh_token: &'a SecretString,
    ) -> CoreResult<Vec<(&'a str, &'a str)>> {
        let mut p = vec![
            ("client_id", self.client_id()?),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose_secret()),
        ];
        if let Some(s) = &self.client_secret {
            p.push(("client_secret", s.expose_secret()));
        }
        Ok(p)
    }

    fn client_id(&self) -> CoreResult<&str> {
        self.info
            .client_id
            .as_deref()
            .ok_or_else(|| CoreError::new(ErrorCode::Unsupported, "Google Drive is not configured"))
    }

    fn parse_file_metadata(
        body_bytes: &[u8],
        fallback_name: &str,
        fallback_size: u64,
    ) -> CoreResult<CloudFileMetadata> {
        let parsed: serde_json::Value =
            serde_json::from_slice(body_bytes).map_err(|e| CoreError::internal(e.to_string()))?;

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
            .and_then(serde_json::Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(fallback_size);
        let created_at = parsed
            .get("createdTime")
            .and_then(serde_json::Value::as_str)
            .and_then(Timestamp::parse_rfc3339);
        let modified_at = parsed
            .get("modifiedTime")
            .and_then(serde_json::Value::as_str)
            .and_then(Timestamp::parse_rfc3339);

        Ok(CloudFileMetadata {
            id,
            name,
            size_bytes,
            created_at,
            modified_at,
            provider: "google_drive".to_string(),
        })
    }

    async fn upload_multipart(
        &self,
        access_token: &SecretString,
        filename: &str,
        path: &Path,
        file_size: u64,
        options: Option<&CloudTransferOptions>,
    ) -> CoreResult<CloudFileMetadata> {
        if let Some(opts) = options
            && opts.cancel.is_cancelled()
        {
            return Err(CoreError::new(
                ErrorCode::Cancelled,
                "Upload cancelled by user",
            ));
        }
        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| CoreError::io("Cannot read file to upload to Google Drive", &e))?;
        let boundary = "mcpanel_drive_boundary_42";
        let metadata = serde_json::json!({
            "name": filename,
            "description": "MCPanel Backup"
        });
        let meta_str =
            serde_json::to_string(&metadata).map_err(|e| CoreError::internal(e.to_string()))?;

        let mut body = Vec::new();
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta_str}\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(&file_bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let req = self
            .http
            .inner()
            .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,name,size,createdTime,modifiedTime")
            .bearer_auth(access_token.expose_secret())
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/related; boundary={boundary}"),
            )
            .timeout(std::time::Duration::from_secs(60))
            .body(body);

        let resp = if let Some(opts) = options {
            tokio::select! {
                _ = opts.cancel.cancelled() => {
                    return Err(CoreError::new(ErrorCode::Cancelled, "Upload cancelled by user"));
                }
                res = req.send() => res.map_err(|e| CoreError::new(ErrorCode::ProviderError, format!("{NAME} upload request failed: {e}")))?,
            }
        } else {
            req.send().await.map_err(|e| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    format!("{NAME} upload request failed: {e}"),
                )
            })?
        };

        let status = resp.status().as_u16();
        let body_bytes = resp.bytes().await.map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} upload read failed: {e}"),
            )
        })?;

        if !(200..300).contains(&status) {
            let err_text = String::from_utf8_lossy(&body_bytes);
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} upload failed (HTTP {status}): {err_text}"),
            ));
        }

        if let Some(opts) = options {
            opts.report(file_size, file_size);
        }

        Self::parse_file_metadata(&body_bytes, filename, file_size)
    }

    async fn upload_resumable(
        &self,
        access_token: &SecretString,
        filename: &str,
        path: &Path,
        file_size: u64,
        chunk_size: usize,
        options: Option<&CloudTransferOptions>,
    ) -> CoreResult<CloudFileMetadata> {
        use tokio::io::AsyncReadExt;
        if let Some(opts) = options
            && opts.cancel.is_cancelled()
        {
            return Err(CoreError::new(
                ErrorCode::Cancelled,
                "Upload cancelled by user",
            ));
        }

        let metadata = serde_json::json!({
            "name": filename,
            "description": "MCPanel Backup"
        });
        let req = self
            .http
            .inner()
            .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id,name,size,createdTime,modifiedTime")
            .bearer_auth(access_token.expose_secret())
            .header("X-Upload-Content-Type", "application/octet-stream")
            .header("X-Upload-Content-Length", file_size.to_string())
            .header(reqwest::header::CONTENT_TYPE, "application/json; charset=UTF-8")
            .timeout(std::time::Duration::from_secs(60))
            .json(&metadata);

        let resp = if let Some(opts) = options {
            tokio::select! {
                _ = opts.cancel.cancelled() => {
                    return Err(CoreError::new(ErrorCode::Cancelled, "Upload cancelled by user"));
                }
                res = req.send() => res.map_err(|e| CoreError::new(ErrorCode::ProviderError, format!("{NAME} resumable session start request failed: {e}")))?,
            }
        } else {
            req.send().await.map_err(|e| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    format!("{NAME} resumable session start request failed: {e}"),
                )
            })?
        };

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} resumable session start failed (HTTP {status}): {err_text}"),
            ));
        }

        let session_url = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    format!("{NAME} missing Location header for resumable upload"),
                )
            })?
            .to_string();

        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|e| CoreError::io("Cannot open file for upload", &e))?;

        let mut uploaded_offset = 0u64;
        let mut buffer = vec![0u8; chunk_size];

        while uploaded_offset < file_size {
            if let Some(opts) = options
                && opts.cancel.is_cancelled()
            {
                let _ = self
                    .http
                    .inner()
                    .delete(&session_url)
                    .bearer_auth(access_token.expose_secret())
                    .header(reqwest::header::CONTENT_LENGTH, "0")
                    .send()
                    .await;
                return Err(CoreError::new(
                    ErrorCode::Cancelled,
                    "Upload cancelled by user",
                ));
            }

            let n = file
                .read(&mut buffer)
                .await
                .map_err(|e| CoreError::io("Cannot read chunk for upload", &e))?;
            if n == 0 {
                break;
            }

            let chunk_end = uploaded_offset + n as u64;
            let range_header = format!("bytes {uploaded_offset}-{}/{file_size}", chunk_end - 1);
            let chunk_bytes = bytes::Bytes::copy_from_slice(&buffer[..n]);

            let req = self
                .http
                .inner()
                .put(&session_url)
                .header(reqwest::header::CONTENT_RANGE, range_header)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .timeout(std::time::Duration::from_secs(60))
                .body(chunk_bytes);

            let resp = if let Some(opts) = options {
                tokio::select! {
                    _ = opts.cancel.cancelled() => {
                        let _ = self
                            .http
                            .inner()
                            .delete(&session_url)
                            .bearer_auth(access_token.expose_secret())
                            .header(reqwest::header::CONTENT_LENGTH, "0")
                            .send()
                            .await;
                        return Err(CoreError::new(ErrorCode::Cancelled, "Upload cancelled by user"));
                    }
                    res = req.send() => res.map_err(|e| CoreError::new(ErrorCode::ProviderError, format!("{NAME} resumable upload chunk failed: {e}")))?,
                }
            } else {
                req.send().await.map_err(|e| {
                    CoreError::new(
                        ErrorCode::ProviderError,
                        format!("{NAME} resumable upload chunk failed: {e}"),
                    )
                })?
            };

            let status = resp.status().as_u16();
            if status == 308 {
                uploaded_offset = chunk_end;
                if let Some(opts) = options {
                    opts.report(uploaded_offset, file_size);
                }
            } else if (200..300).contains(&status) {
                let body_bytes = resp.bytes().await.map_err(|e| {
                    CoreError::new(
                        ErrorCode::ProviderError,
                        format!("{NAME} upload read failed: {e}"),
                    )
                })?;
                if let Some(opts) = options {
                    opts.report(file_size, file_size);
                }
                return Self::parse_file_metadata(&body_bytes, filename, file_size);
            } else {
                let err_text = resp.text().await.unwrap_or_default();
                return Err(CoreError::new(
                    ErrorCode::ProviderError,
                    format!("{NAME} chunk upload failed (HTTP {status}): {err_text}"),
                ));
            }
        }

        Err(CoreError::new(
            ErrorCode::ProviderError,
            format!("{NAME} resumable upload ended prematurely"),
        ))
    }
}

#[derive(Deserialize)]
struct About {
    user: Option<User>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct User {
    display_name: Option<String>,
    email_address: Option<String>,
}

#[async_trait]
impl CloudStorageProvider for GoogleDrive {
    fn info(&self) -> &CloudProviderInfo {
        &self.info
    }

    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> CoreResult<TokenSet> {
        let params = self.exchange_params(code, verifier, redirect_uri)?;
        token_request(&self.http, NAME, Stage::Exchange, TOKEN_URL, &params).await
    }

    async fn refresh(&self, refresh_token: &SecretString) -> CoreResult<TokenSet> {
        let params = self.refresh_params(refresh_token)?;
        token_request(&self.http, NAME, Stage::Refresh, TOKEN_URL, &params).await
    }

    async fn account(&self, access_token: &SecretString) -> CoreResult<CloudAccount> {
        let (status, body) = self
            .http
            .send_raw(
                reqwest::Method::GET,
                "https://www.googleapis.com/drive/v3/about?fields=user(displayName,emailAddress)",
                Some(access_token.expose_secret()),
                None,
            )
            .await?;
        let about: About = api_json(NAME, "account lookup", status, &body)?;
        let user = about.user.unwrap_or(User {
            display_name: None,
            email_address: None,
        });
        Ok(CloudAccount {
            display_name: user.display_name,
            email: user.email_address,
        })
    }

    async fn revoke(&self, refresh_token: &SecretString) -> CoreResult<RevokeOutcome> {
        let (status, _) = self
            .http
            .send_raw(
                reqwest::Method::POST,
                "https://oauth2.googleapis.com/revoke",
                None,
                Some((
                    "application/x-www-form-urlencoded",
                    encode_form(&[("token", refresh_token.expose_secret())]),
                )),
            )
            .await?;
        // 400 means the token is already invalid — nothing left to revoke.
        if (200..300).contains(&status) || status == 400 {
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
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| CoreError::io("Cannot access backup file to upload", &e))?;
        let file_size = meta.len();

        if file_size <= RESUMABLE_UPLOAD_THRESHOLD_BYTES {
            self.upload_multipart(access_token, filename, path, file_size, options.as_ref())
                .await
        } else {
            self.upload_resumable(
                access_token,
                filename,
                path,
                file_size,
                DRIVE_CHUNK_SIZE,
                options.as_ref(),
            )
            .await
        }
    }

    async fn list_files(&self, access_token: &SecretString) -> CoreResult<Vec<CloudFileMetadata>> {
        let resp = self
            .http
            .inner()
            .get("https://www.googleapis.com/drive/v3/files?q=trashed=false&fields=files(id,name,size,createdTime,modifiedTime)&pageSize=100")
            .bearer_auth(access_token.expose_secret())
            .send()
            .await
            .map_err(|e| CoreError::new(ErrorCode::ProviderError, format!("{NAME} list request failed: {e}")))?;

        let status = resp.status().as_u16();
        let body_bytes = resp.bytes().await.map_err(|e| {
            CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} list read failed: {e}"),
            )
        })?;

        if !(200..300).contains(&status) {
            let err_text = String::from_utf8_lossy(&body_bytes);
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} list failed (HTTP {status}): {err_text}"),
            ));
        }

        let parsed: serde_json::Value =
            serde_json::from_slice(&body_bytes).map_err(|e| CoreError::internal(e.to_string()))?;

        let mut out = Vec::new();
        if let Some(files) = parsed.get("files").and_then(serde_json::Value::as_array) {
            for f in files {
                let name = f
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if !name.ends_with(".zip") && !name.ends_with(".zip.age") {
                    continue;
                }
                let id = f
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let size_bytes = f
                    .get("size")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let created_at = f
                    .get("createdTime")
                    .and_then(serde_json::Value::as_str)
                    .and_then(Timestamp::parse_rfc3339);
                let modified_at = f
                    .get("modifiedTime")
                    .and_then(serde_json::Value::as_str)
                    .and_then(Timestamp::parse_rfc3339);

                out.push(CloudFileMetadata {
                    id,
                    name,
                    size_bytes,
                    created_at,
                    modified_at,
                    provider: "google_drive".to_string(),
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

        let url = format!("https://www.googleapis.com/drive/v3/files/{file_id}?alt=media");
        let req = self
            .http
            .inner()
            .get(&url)
            .bearer_auth(access_token.expose_secret())
            .send();

        let resp = if let Some(opts) = &options {
            tokio::select! {
                _ = opts.cancel.cancelled() => {
                    return Err(CoreError::new(ErrorCode::Cancelled, "Download cancelled by user"));
                }
                res = req => res.map_err(|e| {
                    CoreError::new(
                        ErrorCode::ProviderError,
                        format!("{NAME} download request failed: {e}"),
                    )
                })?,
            }
        } else {
            req.await.map_err(|e| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    format!("{NAME} download request failed: {e}"),
                )
            })?
        };

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} download failed (HTTP {status}): {text}"),
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
        let url = format!("https://www.googleapis.com/drive/v3/files/{file_id}");
        let resp = self
            .http
            .inner()
            .delete(&url)
            .bearer_auth(access_token.expose_secret())
            .send()
            .await
            .map_err(|e| {
                CoreError::new(
                    ErrorCode::ProviderError,
                    format!("{NAME} delete request failed: {e}"),
                )
            })?;

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) && status != 404 {
            let text = resp.text().await.unwrap_or_default();
            return Err(CoreError::new(
                ErrorCode::ProviderError,
                format!("{NAME} delete failed (HTTP {status}): {text}"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "GOCSPX-test-only-not-real";

    fn provider(secret: Option<&str>) -> GoogleDrive {
        GoogleDrive::with_secret(
            crate::http_client().unwrap(),
            Some("123-abc.apps.googleusercontent.com".into()),
            secret.map(|s| SecretString::from(s.to_string())),
        )
    }

    #[test]
    fn code_exchange_sends_pkce_verifier_and_client_secret() {
        let g = provider(Some(SECRET));
        let p = g
            .exchange_params("the-code", "verifier-43-chars", "http://127.0.0.1:5000/")
            .unwrap();
        let get = |k: &str| p.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        assert_eq!(get("code_verifier"), Some("verifier-43-chars"));
        assert_eq!(get("client_secret"), Some(SECRET));
        assert_eq!(get("grant_type"), Some("authorization_code"));
        assert_eq!(get("client_id"), Some("123-abc.apps.googleusercontent.com"));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:5000/"));
    }

    #[test]
    fn refresh_sends_client_secret() {
        let g = provider(Some(SECRET));
        let rt = SecretString::from("1//refresh".to_string());
        let p = g.refresh_params(&rt).unwrap();
        let get = |k: &str| p.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        assert_eq!(get("grant_type"), Some("refresh_token"));
        assert_eq!(get("refresh_token"), Some("1//refresh"));
        assert_eq!(get("client_secret"), Some(SECRET));
        // Without a configured secret nothing is invented.
        let none = provider(None);
        let p = none.refresh_params(&rt).unwrap();
        assert!(!p.iter().any(|(k, _)| *k == "client_secret"));
    }

    #[test]
    fn the_secret_never_appears_in_errors_or_debug_output() {
        let e = crate::cloud::parse_token_response(
            NAME,
            Stage::Exchange,
            401,
            br#"{"error":"invalid_client","error_description":"The provided client secret is invalid."}"#,
        )
        .err()
        .unwrap();
        assert!(!e.message.contains(SECRET));
        let ids = crate::cloud::CloudClientIds {
            google_secret: Some(SecretString::from(SECRET.to_string())),
            ..Default::default()
        };
        let dbg = format!("{ids:?}");
        assert!(!dbg.contains(SECRET) && dbg.contains("[set]"), "{dbg}");
        let info = format!("{:?}", provider(Some(SECRET)).info());
        assert!(!info.contains(SECRET), "{info}");
    }

    #[test]
    fn public_desktop_oauth_pkce_without_client_secret() {
        let g = provider(None);
        assert_eq!(
            g.info().extra_authorize_params,
            &[
                ("access_type", "offline"),
                ("prompt", "consent select_account")
            ]
        );
        let p = g
            .exchange_params("the-code", "verifier-43-chars", "http://127.0.0.1:5000/")
            .unwrap();
        let get = |k: &str| p.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        assert_eq!(get("grant_type"), Some("authorization_code"));
        assert_eq!(get("code"), Some("the-code"));
        assert_eq!(get("code_verifier"), Some("verifier-43-chars"));
        assert_eq!(get("client_id"), Some("123-abc.apps.googleusercontent.com"));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:5000/"));
        assert!(!p.iter().any(|(k, _)| *k == "client_secret"));

        let rt = SecretString::from("1//refresh-tok".to_string());
        let ref_params = g.refresh_params(&rt).unwrap();
        let get_ref = |k: &str| ref_params.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        assert_eq!(get_ref("grant_type"), Some("refresh_token"));
        assert_eq!(get_ref("refresh_token"), Some("1//refresh-tok"));
        assert_eq!(
            get_ref("client_id"),
            Some("123-abc.apps.googleusercontent.com")
        );
        assert!(!ref_params.iter().any(|(k, _)| *k == "client_secret"));
    }
}
