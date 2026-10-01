#![allow(clippy::unwrap_used)]
//! Accounts against a fake Firebase backend: email sign-up/sign-in, the cached profile,
//! sign-out, and "Continue with Google" through the real loopback redirect.

use async_trait::async_trait;
use mcpanel_core::account::{
    AccountProfile, AccountService, AuthBackend, AuthSession, GoogleSignIn,
};
use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::{SecretStore, SettingsRepository};
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

/// Firebase as the tests need it: one existing account.
#[derive(Default)]
struct FakeFirebase {
    names: Mutex<HashMap<String, String>>,
    verification_sent: Mutex<u32>,
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
        if email == "taken@example.com" {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                "An account with this email already exists. Sign in instead.",
            ));
        }
        Ok(session("new-user"))
    }
    async fn sign_in(&self, email: &str, password: &str) -> CoreResult<AuthSession> {
        if email == "taken@example.com" && password == "right password" {
            Ok(session("existing"))
        } else {
            Err(CoreError::new(
                ErrorCode::InvalidInput,
                "The email or password is wrong.",
            ))
        }
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
            mcpanel_core::account::AccountType::Google
        } else {
            mcpanel_core::account::AccountType::Email
        };
        Ok(AccountProfile {
            account_id: uid.clone(),
            account_type,
            email: Some(match uid.as_str() {
                "google-user" => "g@gmail.com".into(),
                "existing" => "taken@example.com".into(),
                _ => "new@example.com".into(),
            }),
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
        *self.verification_sent.lock().unwrap() += 1;
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
        challenge: &str,
        state: &str,
    ) -> CoreResult<String> {
        assert_eq!(challenge.len(), 43, "S256 challenge");
        Ok(format!(
            "https://accounts.google.com/o/oauth2/v2/auth?redirect_uri={redirect}&state={state}"
        ))
    }
    async fn google_exchange(
        &self,
        code: &str,
        verifier: &str,
        redirect: &str,
    ) -> CoreResult<SecretString> {
        assert_eq!(code, "google-code");
        assert!(verifier.len() >= 43);
        assert!(redirect.starts_with("http://127.0.0.1:"));
        Ok(SecretString::from("google-id-token".to_string()))
    }
}

fn service() -> (Arc<AccountService>, Arc<MemSecrets>, Arc<FakeFirebase>) {
    let secrets = Arc::new(MemSecrets::default());
    let a = Arc::new(AccountService::new(
        secrets.clone(),
        Arc::new(MemSettings::default()),
    ));
    let fb = Arc::new(FakeFirebase::default());
    a.set_backend(fb.clone());
    (a, secrets, fb)
}

#[tokio::test]
async fn email_accounts() {
    let (a, secrets, fb) = service();
    assert!(!a.status().await.unwrap().signed_in);
    assert!(
        a.sign_up("not-an-email", "long enough", None)
            .await
            .is_err()
    );
    assert!(a.sign_up("x@example.com", "short", None).await.is_err());
    let e = a
        .sign_up("taken@example.com", "long enough", None)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);

    let p = a
        .sign_up("new@example.com", "long enough", Some("Alex"))
        .await
        .unwrap();
    assert_eq!(p.display_name.as_deref(), Some("Alex"));
    assert_eq!(
        *fb.verification_sent.lock().unwrap(),
        1,
        "verification email sent"
    );
    let s = a.status().await.unwrap();
    assert!(s.signed_in);
    assert_eq!(s.profile.unwrap().email.as_deref(), Some("new@example.com"));
    assert_eq!(
        secrets
            .get("account-refresh-token")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "refresh:new-user"
    );

    a.set_display_name("Alex M").await.unwrap();
    assert_eq!(
        a.status()
            .await
            .unwrap()
            .profile
            .unwrap()
            .display_name
            .as_deref(),
        Some("Alex M")
    );
    a.resend_verification().await.unwrap();
    assert_eq!(*fb.verification_sent.lock().unwrap(), 2);

    a.sign_out().await.unwrap();
    let s = a.status().await.unwrap();
    assert!(!s.signed_in && s.profile.is_none());
    assert!(secrets.get("account-refresh-token").unwrap().is_none());

    assert!(a.sign_in("taken@example.com", "wrong").await.is_err());
    let p = a
        .sign_in("taken@example.com", "right password")
        .await
        .unwrap();
    assert_eq!(p.uid, "existing");
}

#[tokio::test]
async fn continue_with_google_through_the_loopback_redirect() {
    let (a, _, _) = service();
    let url = a.begin_google().await.unwrap();
    assert_eq!(
        a.begin_google().await.unwrap(),
        url,
        "pending sign-in is reused"
    );
    let redirect = url
        .split("redirect_uri=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let state = url.split("state=").nth(1).unwrap().to_string();
    // The browser comes back from Google.
    let addr = redirect
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    let mut sock = tokio::net::TcpStream::connect(&addr).await.unwrap();
    sock.write_all(
        format!("GET /?code=google-code&state={state} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut page = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), sock.read_to_end(&mut page)).await;
    let mut done = false;
    for _ in 0..50 {
        if a.status().await.unwrap().google == Some(GoogleSignIn::Done) {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(done, "{:?}", a.status().await.unwrap().google);
    let p = a.status().await.unwrap().profile.unwrap();
    assert_eq!(p.provider, "google.com");
    assert!(p.email_verified);
}

#[tokio::test]
async fn a_forged_redirect_is_rejected() {
    let (a, _, _) = service();
    let url = a.begin_google().await.unwrap();
    let redirect = url
        .split("redirect_uri=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let addr = redirect
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    let mut sock = tokio::net::TcpStream::connect(&addr).await.unwrap();
    sock.write_all(
        format!("GET /?code=google-code&state=not-the-state HTTP/1.1\r\nHost: {addr}\r\n\r\n")
            .as_bytes(),
    )
    .await
    .unwrap();
    let mut page = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), sock.read_to_end(&mut page)).await;
    for _ in 0..50 {
        if matches!(
            a.status().await.unwrap().google,
            Some(GoogleSignIn::Failed { .. })
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(matches!(
        a.status().await.unwrap().google,
        Some(GoogleSignIn::Failed { .. })
    ));
    assert!(!a.status().await.unwrap().signed_in);
}

#[tokio::test]
async fn without_a_firebase_project_accounts_are_unavailable() {
    let a = AccountService::new(
        Arc::new(MemSecrets::default()),
        Arc::new(MemSettings::default()),
    );
    let s = a.status().await.unwrap();
    assert!(!s.configured);
    let e = a.sign_in("x@example.com", "long enough").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
}

#[tokio::test]
async fn guest_mode_lifecycle_and_migration() {
    let (a, _, _) = service();

    // Initially not signed in, not guest
    let s0 = a.status().await.unwrap();
    assert!(!s0.signed_in);
    assert!(!s0.is_guest);
    assert!(s0.profile.is_none());

    // Enter guest mode
    let guest_prof = a.enter_guest_mode().await.unwrap();
    assert_eq!(guest_prof.uid, "guest");
    assert!(guest_prof.is_guest());

    let s1 = a.status().await.unwrap();
    assert!(!s1.signed_in);
    assert!(s1.is_guest);
    assert_eq!(s1.profile.as_ref().unwrap().account_id, "guest");

    // Upgrade / sign in from guest mode
    let prof = a
        .sign_in("taken@example.com", "right password")
        .await
        .unwrap();
    assert_eq!(prof.uid, "existing");
    assert!(!prof.is_guest());

    // Guest mode should be cleared after authenticated sign in
    let s2 = a.status().await.unwrap();
    assert!(s2.signed_in);
    assert!(!s2.is_guest);
    assert_eq!(s2.profile.as_ref().unwrap().uid, "existing");

    // Sign out clears both authenticated and guest session
    a.sign_out().await.unwrap();
    let s3 = a.status().await.unwrap();
    assert!(!s3.signed_in);
    assert!(!s3.is_guest);
    assert!(s3.profile.is_none());
}
