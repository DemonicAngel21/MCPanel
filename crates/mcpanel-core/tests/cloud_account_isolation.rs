#![allow(clippy::unwrap_used)]
//! Tests for Cloud Storage account isolation, guest protection, and Google auto-linking.

use async_trait::async_trait;
use mcpanel_core::account::{
    AccountProfile, AccountService, AccountType, AuthBackend, AuthSession, GoogleAuthTokens,
};
use mcpanel_core::cloud::{
    CloudAccount, CloudProviderInfo, CloudService, CloudStorageProvider, RedirectSpec,
    RevokeOutcome, TokenSet,
};
use mcpanel_core::error::{CoreResult, ErrorCode};
use mcpanel_core::events::EventBus;
use mcpanel_core::ports::{SecretStore, SettingsRepository};
use mcpanel_core::time::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct MemSettings(Mutex<HashMap<String, serde_json::Value>>);

#[async_trait]
impl SettingsRepository for MemSettings {
    async fn get(&self, key: &str) -> CoreResult<Option<serde_json::Value>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .filter(|v| !v.is_null()))
    }
    async fn set(&self, key: &str, value: &serde_json::Value) -> CoreResult<()> {
        self.0.lock().unwrap().insert(key.into(), value.clone());
        Ok(())
    }
    async fn all(&self) -> CoreResult<Vec<(String, serde_json::Value)>> {
        Ok(self.0.lock().unwrap().clone().into_iter().collect())
    }
}

#[derive(Default)]
struct MemSecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemSecrets {
    fn get(&self, name: &str) -> CoreResult<Option<SecretString>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .map(SecretString::from))
    }
    fn set(&self, name: &str, value: &SecretString) -> CoreResult<()> {
        self.0
            .lock()
            .unwrap()
            .insert(name.into(), value.expose_secret().to_string());
        Ok(())
    }
    fn delete(&self, name: &str) -> CoreResult<()> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

struct FakeProvider {
    info: CloudProviderInfo,
}

#[async_trait]
impl CloudStorageProvider for FakeProvider {
    fn info(&self) -> &CloudProviderInfo {
        &self.info
    }
    async fn exchange_code(
        &self,
        _code: &str,
        _verifier: &str,
        _redirect: &str,
    ) -> CoreResult<TokenSet> {
        Ok(TokenSet {
            access_token: SecretString::from("at-123"),
            refresh_token: Some(SecretString::from("rt-123")),
            expires_at: Some(Timestamp(Timestamp::now().millis() + 3_600_000)),
        })
    }
    async fn refresh(&self, _rt: &SecretString) -> CoreResult<TokenSet> {
        Ok(TokenSet {
            access_token: SecretString::from("at-refreshed"),
            refresh_token: Some(SecretString::from("rt-refreshed")),
            expires_at: Some(Timestamp(Timestamp::now().millis() + 3_600_000)),
        })
    }
    async fn account(&self, _at: &SecretString) -> CoreResult<CloudAccount> {
        Ok(CloudAccount {
            display_name: Some("Test User".into()),
            email: Some("test@example.com".into()),
        })
    }
    async fn revoke(&self, _rt: &SecretString) -> CoreResult<RevokeOutcome> {
        Ok(RevokeOutcome::Revoked)
    }
}

#[derive(Default)]
struct FakeFirebase {
    names: Mutex<HashMap<String, String>>,
}

fn session(uid: &str) -> AuthSession {
    AuthSession {
        id_token: SecretString::from(format!("id:{uid}")),
        refresh_token: SecretString::from(format!("refresh:{uid}")),
    }
}

#[async_trait]
impl AuthBackend for FakeFirebase {
    fn configured(&self) -> bool {
        true
    }
    fn google_available(&self) -> bool {
        true
    }
    async fn sign_up(&self, email: &str, _password: &str) -> CoreResult<AuthSession> {
        let uid = email.split('@').next().unwrap_or("user");
        Ok(session(uid))
    }
    async fn sign_in(&self, email: &str, _password: &str) -> CoreResult<AuthSession> {
        let uid = email.split('@').next().unwrap_or("user");
        Ok(session(uid))
    }
    async fn sign_in_with_google(&self, t: &SecretString) -> CoreResult<AuthSession> {
        assert_eq!(t.expose_secret(), "google-id-token");
        Ok(session("google-user"))
    }
    async fn refresh(&self, r: &SecretString) -> CoreResult<AuthSession> {
        let uid = r.expose_secret().trim_start_matches("refresh:").to_string();
        Ok(session(&uid))
    }
    async fn lookup(&self, id: &SecretString) -> CoreResult<AccountProfile> {
        let uid = id.expose_secret().trim_start_matches("id:").to_string();
        let account_type = if uid == "google-user" {
            AccountType::Google
        } else {
            AccountType::Email
        };
        Ok(AccountProfile {
            account_id: uid.clone(),
            account_type,
            email: Some(format!("{uid}@example.com")),
            email_verified: uid == "google-user",
            display_name: self.names.lock().unwrap().get(&uid).cloned(),
            photo_url: None,
            provider: if uid == "google-user" {
                "google.com"
            } else {
                "password"
            }
            .into(),
            uid,
        })
    }
    async fn send_verification(&self, _id: &SecretString) -> CoreResult<()> {
        Ok(())
    }
    async fn send_password_reset(&self, _email: &str) -> CoreResult<()> {
        Ok(())
    }
    async fn set_display_name(&self, id: &SecretString, name: &str) -> CoreResult<()> {
        let uid = id.expose_secret().trim_start_matches("id:").to_string();
        self.names.lock().unwrap().insert(uid, name.into());
        Ok(())
    }
    fn google_authorize_url(
        &self,
        redirect: &str,
        _challenge: &str,
        state: &str,
    ) -> CoreResult<String> {
        Ok(format!(
            "https://accounts.google.com/o/oauth2/v2/auth?redirect_uri={redirect}&state={state}"
        ))
    }
    async fn google_exchange(
        &self,
        _code: &str,
        _verifier: &str,
        _redirect: &str,
    ) -> CoreResult<GoogleAuthTokens> {
        Ok(GoogleAuthTokens {
            id_token: SecretString::from("id:google-user".to_string()),
            refresh_token: Some(SecretString::from("google-refresh-token".to_string())),
            access_token: Some(SecretString::from("google-access-token".to_string())),
        })
    }
}

fn setup() -> (
    Arc<AccountService>,
    Arc<CloudService>,
    Arc<MemSecrets>,
    Arc<MemSettings>,
) {
    let secrets = Arc::new(MemSecrets::default());
    let settings = Arc::new(MemSettings::default());

    let provider = Arc::new(FakeProvider {
        info: CloudProviderInfo {
            id: "google_drive",
            display_name: "Google Drive",
            client_id: Some("drive-client-id".into()),
            client_id_variable: "MCPANEL_GOOGLE_CLIENT_ID",
            authorize_url: "https://auth.google.example",
            scopes: &["drive.file"],
            extra_authorize_params: &[],
            redirect: RedirectSpec::AnyPort {
                host: "127.0.0.1",
                path: "/google-drive/callback",
            },
            manage_access_url: "https://myaccount.google.com/permissions",
        },
    });

    let cloud = CloudService::new(
        vec![provider],
        Arc::clone(&secrets) as Arc<dyn SecretStore>,
        Arc::clone(&settings) as Arc<dyn SettingsRepository>,
        EventBus::default(),
    );

    let account = Arc::new(AccountService::new(
        Arc::clone(&secrets) as Arc<dyn SecretStore>,
        Arc::clone(&settings) as Arc<dyn SettingsRepository>,
    ));
    account.set_backend(Arc::new(FakeFirebase::default()));

    account.set_cloud(Arc::clone(&cloud));
    cloud.set_account_service(Arc::clone(&account));

    (account, cloud, secrets, settings)
}

#[tokio::test]
async fn guest_cannot_use_cloud_storage() {
    let (account, cloud, _, _) = setup();
    account.enter_guest_mode().await.unwrap();

    let st = cloud.status("google_drive").await.unwrap();
    assert!(st.is_guest);
    assert!(!st.connected);

    let err = cloud.begin_connect("google_drive").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);

    let err = cloud.access_token("google_drive").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
}

#[tokio::test]
async fn account_scoped_cloud_tokens_and_logout_isolation() {
    let (account, cloud, _, _) = setup();

    // Alice logs in with email
    account.sign_in("alice@example.com", "pw").await.unwrap();

    let st = cloud.status("google_drive").await.unwrap();
    assert!(!st.is_guest);
    assert!(!st.connected);

    // Alice connects Google Drive
    cloud
        .link_google_drive(
            "alice",
            &SecretString::from("alice-drive-refresh"),
            Some(&SecretString::from("at-123")),
        )
        .await
        .unwrap();

    let st = cloud.status("google_drive").await.unwrap();
    assert!(st.connected);
    assert_eq!(
        st.account.as_ref().unwrap().email.as_deref(),
        Some("test@example.com")
    );

    // Alice can obtain an access token
    let token = cloud.access_token("google_drive").await.unwrap();
    assert_eq!(token.expose_secret(), "at-refreshed");

    // Alice logs out
    account.sign_out().await.unwrap();

    // Google Drive is immediately disconnected for the session
    let st = cloud.status("google_drive").await.unwrap();
    assert!(!st.connected);

    // Bob logs in with email
    account.sign_in("bob@example.com", "pw").await.unwrap();

    // Bob does NOT have Alice's Google Drive connection
    let bob_st = cloud.status("google_drive").await.unwrap();
    assert!(!bob_st.connected);

    let err = cloud.access_token("google_drive").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn google_auth_auto_links_and_unlinks_on_logout() {
    let (account, cloud, secrets, _) = setup();

    // User signs in with Google
    let (tokens, profile) = {
        let fb = FakeFirebase::default();
        let tokens = fb.google_exchange("code", "v", "r").await.unwrap();
        let p = fb.lookup(&tokens.id_token).await.unwrap();
        (tokens, p)
    };
    account
        .sign_in("google-user@example.com", "password")
        .await
        .unwrap();
    cloud
        .link_google_drive(
            &profile.uid,
            tokens.refresh_token.as_ref().unwrap(),
            tokens.access_token.as_ref(),
        )
        .await
        .unwrap();

    // Google Drive is automatically connected and auto_linked
    let st = cloud.status("google_drive").await.unwrap();
    assert!(st.connected);
    assert!(st.auto_linked);
    assert_eq!(
        secrets
            .get("cloud-google-user-google_drive-refresh-token")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "google-refresh-token"
    );

    // Logging out immediately clears connection
    account.sign_out().await.unwrap();
    let st = cloud.status("google_drive").await.unwrap();
    assert!(!st.connected);
    assert!(!st.auto_linked);
}
