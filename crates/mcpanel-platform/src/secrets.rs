//! `SecretStore` backed by the Windows Credential Manager (generic credentials,
//! persisted for this user on this computer only — `CRED_PERSIST_LOCAL_MACHINE`; blobs
//! are limited to 2560 bytes, verified 2026-09-29 against the `CREDENTIALW` docs).
#![allow(unsafe_code)]

use mcpanel_core::error::{CoreError, CoreResult, ErrorCode};
use mcpanel_core::ports::SecretStore;
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretString};
use windows::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows::Win32::Security::Credentials::{
    CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
    CredReadW, CredWriteW,
};
use windows::core::{PCWSTR, PWSTR};

const MAX_BLOB: usize = 2560;

pub struct CredentialStore {
    /// Target name prefix, e.g. `MCPanel/`; development instances use their own.
    prefix: String,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn err(context: &str, e: impl std::fmt::Display) -> CoreError {
    CoreError::new(ErrorCode::Io, format!("{context}: {e}"))
}

impl CredentialStore {
    /// `namespace` separates stores (the installed app uses `MCPanel`).
    pub fn new(namespace: &str) -> Self {
        Self {
            prefix: format!("{namespace}/"),
        }
    }

    fn target(&self, name: &str) -> Vec<u16> {
        wide(&format!("{}{name}", self.prefix))
    }
}

impl SecretStore for CredentialStore {
    fn get(&self, name: &str) -> CoreResult<Option<SecretString>> {
        let target = self.target(name);
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated target; on success `cred` is freed with CredFree.
        let ok = unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut cred) };
        if let Err(e) = ok {
            // SAFETY: reading the thread's last error right after the failing call.
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(err("Cannot read from the Windows Credential Manager", e));
        }
        // SAFETY: CredReadW succeeded, so `cred` points to a valid CREDENTIALW whose blob
        // has `CredentialBlobSize` bytes (the pointer may be null when the size is 0).
        let mut value = unsafe {
            let c = &*cred;
            let bytes = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(cred as *const _);
            bytes
        };
        let text = std::str::from_utf8(&value).map(str::to_owned);
        value.zeroize();
        let s = text.map_err(|_| err("Credential Manager", "stored secret is not text"))?;
        Ok(Some(SecretString::from(s)))
    }

    fn set(&self, name: &str, value: &SecretString) -> CoreResult<()> {
        let mut blob = value.expose_secret().as_bytes().to_vec();
        if blob.len() > MAX_BLOB {
            return Err(CoreError::invalid(
                "Secret too large for the Credential Manager",
            ));
        }
        let mut target = self.target(name);
        let mut user = wide("MCPanel");
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(user.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: all pointers reference live buffers for the duration of the call.
        let r = unsafe { CredWriteW(&cred, 0) };
        // Best effort: do not leave the plaintext copy in our heap.
        blob.zeroize();
        r.map_err(|e| err("Cannot write to the Windows Credential Manager", e))
    }

    fn delete(&self, name: &str) -> CoreResult<()> {
        let target = self.target(name);
        // SAFETY: valid NUL-terminated target.
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            // SAFETY: reading the thread's last error right after the failing call.
            Err(_) if unsafe { GetLastError() } == ERROR_NOT_FOUND => Ok(()),
            Err(e) => Err(err("Cannot delete from the Windows Credential Manager", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes, reads and deletes a uniquely named test credential in the current
    /// user's Credential Manager.
    #[test]
    fn round_trip_in_the_credential_manager() {
        let ns = format!("MCPanel-test-{}", std::process::id());
        let store = CredentialStore::new(&ns);
        let name = format!("k{}", rand_suffix());
        assert!(store.get(&name).unwrap().is_none());
        store
            .set(
                &name,
                &SecretString::from("AGE-SECRET-KEY-1TEST".to_string()),
            )
            .unwrap();
        assert_eq!(
            store.get(&name).unwrap().unwrap().expose_secret(),
            "AGE-SECRET-KEY-1TEST"
        );
        store.delete(&name).unwrap();
        assert!(store.get(&name).unwrap().is_none());
        store.delete(&name).unwrap();
        assert!(
            store
                .set(&name, &SecretString::from("x".repeat(MAX_BLOB + 1)))
                .is_err()
        );
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }
}
