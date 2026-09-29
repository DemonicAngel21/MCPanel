//! Backup encryption (spec §9): age with an X25519 identity (the Backup Master Key)
//! kept in the OS secret store, and a Recovery Kit — the identity encrypted with the
//! user's passphrase (age scrypt), ASCII-armored — that must be saved during setup.
//! No custom cryptography: everything is the `age` crate's file format.

use crate::error::{CoreError, CoreResult, ErrorCode};
use crate::events::{DomainEvent, EventBus};
use crate::ports::{SecretStore, SettingsRepository};
use crate::time::Timestamp;
use age::secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

/// Secret store entry of the Backup Master Key.
pub const MASTER_KEY: &str = "backup-master-key";
const META_KEY: &str = "encryption";
const ARMOR_BEGIN: &str = "-----BEGIN AGE ENCRYPTED FILE-----";
pub const MIN_PASSPHRASE_CHARS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct KeyMeta {
    /// The public key (`age1…`) new backups are encrypted to.
    recipient: String,
    created_at: Timestamp,
    encrypt_backups: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptionStatus {
    pub configured: bool,
    /// The key's public part (safe to show; identifies which kit belongs to it).
    pub recipient: Option<String>,
    pub created_at: Option<Timestamp>,
    pub encrypt_backups: bool,
    /// The private key is present in this computer's secret store.
    pub key_available: bool,
}

pub struct EncryptionService {
    secrets: Arc<dyn SecretStore>,
    settings: Arc<dyn SettingsRepository>,
    events: EventBus,
}

fn crypto_err(context: &str, e: impl std::fmt::Display) -> CoreError {
    CoreError::new(ErrorCode::Internal, format!("{context}: {e}"))
}

/// The Recovery Kit text for an identity (armored scrypt age file with a readable header).
fn recovery_kit(identity: &age::x25519::Identity, passphrase: SecretString) -> CoreResult<String> {
    let recipient = identity.to_public().to_string();
    let mut armored = Vec::new();
    {
        let enc = age::Encryptor::with_user_passphrase(passphrase);
        let aw =
            age::armor::ArmoredWriter::wrap_output(&mut armored, age::armor::Format::AsciiArmor)
                .map_err(|e| crypto_err("Cannot create the Recovery Kit", e))?;
        let mut w = enc
            .wrap_output(aw)
            .map_err(|e| crypto_err("Cannot create the Recovery Kit", e))?;
        w.write_all(identity.to_string().expose_secret().as_bytes())
            .map_err(|e| crypto_err("Cannot create the Recovery Kit", e))?;
        w.finish()
            .and_then(|aw| aw.finish())
            .map_err(|e| crypto_err("Cannot create the Recovery Kit", e))?;
    }
    let armored = String::from_utf8(armored).map_err(|e| crypto_err("Recovery Kit", e))?;
    Ok(format!(
        "MCPanel backup Recovery Kit\r\n\
         Key: {recipient}\r\n\
         \r\n\
         This file and your passphrase restore access to encrypted MCPanel backups, for\r\n\
         example on a new computer (Settings > Backup encryption > Import Recovery Kit).\r\n\
         Keep both safe and separate. Without them, encrypted backups cannot be opened.\r\n\
         \r\n\
         {armored}"
    ))
}

/// Recover the identity from a Recovery Kit and its passphrase.
fn open_kit(text: &str, passphrase: SecretString) -> CoreResult<age::x25519::Identity> {
    let start = text
        .find(ARMOR_BEGIN)
        .ok_or_else(|| CoreError::invalid("This file is not an MCPanel Recovery Kit"))?;
    let reader = age::armor::ArmoredReader::new(&text.as_bytes()[start..]);
    let dec = age::Decryptor::new(reader)
        .map_err(|_| CoreError::invalid("This file is not an MCPanel Recovery Kit"))?;
    let identity = age::scrypt::Identity::new(passphrase);
    let mut r = dec
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| CoreError::invalid("Wrong passphrase for this Recovery Kit"))?;
    let mut s = String::new();
    r.read_to_string(&mut s)
        .map_err(|_| CoreError::invalid("The Recovery Kit is damaged"))?;
    let s = SecretString::from(s);
    age::x25519::Identity::from_str(s.expose_secret().trim())
        .map_err(|_| CoreError::invalid("The Recovery Kit is damaged"))
}

/// Encrypt `src` to `dst` for `recipient` (streaming; checks `cancelled` per chunk).
pub fn encrypt_file(
    src: &Path,
    dst: &Path,
    recipient: &str,
    cancelled: &dyn Fn() -> bool,
) -> CoreResult<()> {
    let r = age::x25519::Recipient::from_str(recipient)
        .map_err(|e| crypto_err("Invalid encryption key", e))?;
    let enc = age::Encryptor::with_recipients(std::iter::once(&r as &dyn age::Recipient))
        .map_err(|e| crypto_err("Cannot encrypt", e))?;
    let input =
        std::fs::File::open(src).map_err(|e| CoreError::io("Cannot read the archive", &e))?;
    let out =
        std::fs::File::create(dst).map_err(|e| CoreError::io("Cannot write the backup", &e))?;
    let mut w = enc
        .wrap_output(BufWriter::new(out))
        .map_err(|e| CoreError::io("Cannot write the backup", &e))?;
    copy_chunks(&mut BufReader::new(input), &mut w, cancelled)?;
    let mut inner = w
        .finish()
        .map_err(|e| CoreError::io("Cannot write the backup", &e))?;
    inner
        .flush()
        .map_err(|e| CoreError::io("Cannot write the backup", &e))?;
    inner
        .into_inner()
        .map_err(|e| CoreError::io("Cannot write the backup", e.error()))?
        .sync_all()
        .map_err(|e| CoreError::io("Cannot write the backup", &e))
}

/// Decrypt `src` to `dst` with `identity` (streaming; authenticated by age).
pub fn decrypt_file(
    src: &Path,
    dst: &Path,
    identity: &SecretString,
    cancelled: &dyn Fn() -> bool,
) -> CoreResult<()> {
    let id = age::x25519::Identity::from_str(identity.expose_secret())
        .map_err(|e| crypto_err("Invalid encryption key", e))?;
    let input =
        std::fs::File::open(src).map_err(|e| CoreError::io("Cannot read the backup", &e))?;
    let dec = age::Decryptor::new_buffered(BufReader::new(input)).map_err(|_| {
        CoreError::new(
            ErrorCode::ArchiveRejected,
            "The backup is not an age-encrypted file",
        )
    })?;
    let mut r = dec
        .decrypt(std::iter::once(&id as &dyn age::Identity))
        .map_err(|_| {
            CoreError::new(
                ErrorCode::ArchiveRejected,
                "This backup was encrypted with a different key. Import the Recovery Kit it belongs to.",
            )
        })?;
    let out = std::fs::File::create(dst)
        .map_err(|e| CoreError::io("Cannot write a temporary file", &e))?;
    let mut w = BufWriter::new(out);
    copy_chunks(&mut r, &mut w, cancelled).map_err(|e| {
        if e.code == ErrorCode::Io {
            CoreError::new(
                ErrorCode::ArchiveRejected,
                "The encrypted backup is damaged",
            )
        } else {
            e
        }
    })?;
    w.flush()
        .map_err(|e| CoreError::io("Cannot write a temporary file", &e))
}

fn copy_chunks(
    r: &mut dyn Read,
    w: &mut dyn Write,
    cancelled: &dyn Fn() -> bool,
) -> CoreResult<()> {
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if cancelled() {
            return Err(CoreError::new(ErrorCode::Cancelled, "Cancelled"));
        }
        let n = r
            .read(&mut buf)
            .map_err(|e| CoreError::io("Cannot read", &e))?;
        if n == 0 {
            return Ok(());
        }
        w.write_all(&buf[..n])
            .map_err(|e| CoreError::io("Cannot write", &e))?;
    }
}

impl EncryptionService {
    pub fn new(
        secrets: Arc<dyn SecretStore>,
        settings: Arc<dyn SettingsRepository>,
        events: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            secrets,
            settings,
            events,
        })
    }

    async fn meta(&self) -> CoreResult<Option<KeyMeta>> {
        Ok(self
            .settings
            .get(META_KEY)
            .await?
            .and_then(|v| serde_json::from_value(v).ok()))
    }

    async fn save_meta(&self, m: &KeyMeta) -> CoreResult<()> {
        let v = serde_json::to_value(m).map_err(|e| CoreError::internal(e.to_string()))?;
        self.settings.set(META_KEY, &v).await?;
        self.events.publish(DomainEvent::SettingsChanged {
            key: META_KEY.into(),
        });
        Ok(())
    }

    pub async fn status(&self) -> CoreResult<EncryptionStatus> {
        let meta = self.meta().await?;
        let key_available = self.secrets.get(MASTER_KEY)?.is_some();
        Ok(EncryptionStatus {
            configured: meta.is_some(),
            recipient: meta.as_ref().map(|m| m.recipient.clone()),
            created_at: meta.as_ref().map(|m| m.created_at),
            encrypt_backups: meta.as_ref().is_some_and(|m| m.encrypt_backups) && key_available,
            key_available,
        })
    }

    /// Create the Backup Master Key and write its Recovery Kit to `kit_dest`. The key is
    /// stored only after the kit was written.
    pub async fn setup(
        &self,
        passphrase: SecretString,
        kit_dest: &Path,
    ) -> CoreResult<EncryptionStatus> {
        if self.meta().await?.is_some() {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                "Backup encryption is already set up",
            ));
        }
        if passphrase.expose_secret().chars().count() < MIN_PASSPHRASE_CHARS {
            return Err(CoreError::invalid(format!(
                "Use a passphrase of at least {MIN_PASSPHRASE_CHARS} characters"
            )));
        }
        let identity = age::x25519::Identity::generate();
        let dest = kit_dest.to_path_buf();
        let id2 = identity.clone();
        tokio::task::spawn_blocking(move || -> CoreResult<()> {
            let kit = recovery_kit(&id2, passphrase)?;
            crate::files::fsx::atomic_write(&dest, kit.as_bytes())
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))??;
        self.secrets.set(MASTER_KEY, &identity.to_string())?;
        self.save_meta(&KeyMeta {
            recipient: identity.to_public().to_string(),
            created_at: Timestamp::now(),
            encrypt_backups: true,
        })
        .await?;
        self.status().await
    }

    /// Restore the key from a Recovery Kit (e.g. on a new computer).
    pub async fn import(
        &self,
        kit_path: &Path,
        passphrase: SecretString,
    ) -> CoreResult<EncryptionStatus> {
        let path = kit_path.to_path_buf();
        let identity = tokio::task::spawn_blocking(move || -> CoreResult<age::x25519::Identity> {
            let md =
                std::fs::metadata(&path).map_err(|e| CoreError::io("Cannot read the file", &e))?;
            if md.len() > 64 * 1024 {
                return Err(CoreError::invalid(
                    "This file is not an MCPanel Recovery Kit",
                ));
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|_| CoreError::invalid("This file is not an MCPanel Recovery Kit"))?;
            open_kit(&text, passphrase)
        })
        .await
        .map_err(|e| CoreError::internal(e.to_string()))??;
        let recipient = identity.to_public().to_string();
        let meta = self.meta().await?;
        if let Some(m) = &meta
            && m.recipient != recipient
            && self.secrets.get(MASTER_KEY)?.is_some()
        {
            return Err(CoreError::new(
                ErrorCode::Conflict,
                "A different encryption key is already set up on this computer",
            ));
        }
        self.secrets.set(MASTER_KEY, &identity.to_string())?;
        self.save_meta(&KeyMeta {
            recipient,
            created_at: meta.as_ref().map_or_else(Timestamp::now, |m| m.created_at),
            encrypt_backups: meta.as_ref().is_none_or(|m| m.encrypt_backups),
        })
        .await?;
        self.status().await
    }

    pub async fn set_encrypt_backups(&self, on: bool) -> CoreResult<EncryptionStatus> {
        let mut m = self
            .meta()
            .await?
            .ok_or_else(|| CoreError::invalid("Set up backup encryption first"))?;
        if on && self.secrets.get(MASTER_KEY)?.is_none() {
            return Err(CoreError::invalid(
                "The encryption key is not on this computer. Import your Recovery Kit first.",
            ));
        }
        m.encrypt_backups = on;
        self.save_meta(&m).await?;
        self.status().await
    }

    /// The public key for new backups, when encryption is on.
    pub async fn backup_recipient(&self) -> CoreResult<Option<String>> {
        let s = self.status().await?;
        Ok(s.encrypt_backups.then_some(s.recipient).flatten())
    }

    /// The private key, for decrypting backups.
    pub fn identity(&self) -> CoreResult<SecretString> {
        self.secrets.get(MASTER_KEY)?.ok_or_else(|| {
            CoreError::new(
                ErrorCode::NotFound,
                "The encryption key is not on this computer. Import your Recovery Kit (Settings > Backup encryption).",
            )
        })
    }
}

/// In-memory secret store (tests and development builds without an OS store).
#[derive(Default)]
pub struct MemorySecretStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl SecretStore for MemorySecretStore {
    fn get(&self, name: &str) -> CoreResult<Option<SecretString>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .map(|s| SecretString::from(s.clone())))
    }

    fn set(&self, name: &str, value: &SecretString) -> CoreResult<()> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.into(), value.expose_secret().to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> CoreResult<()> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn recovery_kit_round_trip() {
        let id = age::x25519::Identity::generate();
        let kit = recovery_kit(&id, pw("correct horse battery")).unwrap();
        assert!(kit.starts_with("MCPanel backup Recovery Kit"));
        assert!(kit.contains(&id.to_public().to_string()));
        assert!(!kit.contains(id.to_string().expose_secret()));
        let Ok(back) = open_kit(&kit, pw("correct horse battery")) else {
            panic!("kit does not open")
        };
        assert_eq!(back.to_public().to_string(), id.to_public().to_string());
        let Err(e) = open_kit(&kit, pw("wrong passphrase!!")) else {
            panic!("wrong passphrase accepted")
        };
        assert_eq!(e.message, "Wrong passphrase for this Recovery Kit");
        assert!(open_kit("hello", pw("x")).is_err());
    }

    #[test]
    fn files_round_trip_and_need_the_right_key() {
        let d = tempfile::tempdir().unwrap();
        let plain = d.path().join("a.zip");
        let data: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&plain, &data).unwrap();
        let id = age::x25519::Identity::generate();
        let enc = d.path().join("a.zip.age");
        encrypt_file(&plain, &enc, &id.to_public().to_string(), &|| false).unwrap();
        assert_ne!(std::fs::read(&enc).unwrap()[..20], data[..20]);
        let out = d.path().join("b.zip");
        decrypt_file(&enc, &out, &id.to_string(), &|| false).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), data);

        let other = age::x25519::Identity::generate();
        let e = decrypt_file(&enc, &out, &other.to_string(), &|| false).unwrap_err();
        assert_eq!(e.code, ErrorCode::ArchiveRejected);
        // Tampering is detected.
        let mut bytes = std::fs::read(&enc).unwrap();
        let n = bytes.len();
        bytes[n - 100] ^= 1;
        std::fs::write(&enc, bytes).unwrap();
        assert!(decrypt_file(&enc, &out, &id.to_string(), &|| false).is_err());
        assert!(encrypt_file(&plain, &enc, "age1invalid", &|| false).is_err());
    }
}
