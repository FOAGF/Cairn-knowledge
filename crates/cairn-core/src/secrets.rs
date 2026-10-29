//! API keys in the OS secure store (NFR-SEC-1): Secret Service on Linux, Credential Manager on
//! Windows, through the `keyring` crate (4.x). Keys are stored under the provider id and never
//! touch the database, logs or exports (FR-DATA-5).
//!
//! On Linux the `keyring` crate talks to the Secret Service over `zbus` (pure Rust), so no extra
//! system packages are needed at build time; at run time GNOME Keyring or KWallet must be running.

use crate::{CoreError, Result};

/// Where API keys live. The app uses [`OsKeychain`]; tests use [`MemoryStore`].
pub trait SecretStore: Send + Sync {
    /// The secret saved for `account` (a provider id), if any.
    fn get(&self, account: &str) -> Result<Option<String>>;
    /// Saves or replaces the secret for `account`.
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    /// Removes the secret for `account`. Removing a missing one is not an error.
    fn delete(&self, account: &str) -> Result<()>;
}

/// The OS secure store. `service` separates installs: `cairn` for release builds, `cairn-dev` for
/// development builds, so the two never share keys (NFR-SEC-1).
///
/// If no secure store is running, `set` fails with `CoreError::Invalid` and a message that says so
/// (there is no plain-text fallback); `get` then returns `Ok(None)`.
pub struct OsKeychain {
    service: String,
}

impl OsKeychain {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, account).map_err(|e| keychain_unavailable(&e))
    }
}

/// The message when the OS secure store can't be used (NFR-SEC-1: no plain-text fallback).
fn keychain_unavailable(e: &keyring::Error) -> CoreError {
    tracing::warn!(error = %e, "system keychain unavailable");
    CoreError::Invalid(
        "No system keychain is available, so the key can't be saved. \
         On Linux, GNOME Keyring or KWallet must be running."
            .into(),
    )
}

impl SecretStore for OsKeychain {
    fn get(&self, account: &str) -> Result<Option<String>> {
        // Reading never fails the caller: "no entry" and "no keychain" both mean "no key".
        let Ok(entry) = self.entry(account) else {
            return Ok(None);
        };
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => {
                tracing::warn!(error = %e, "could not read a key from the system keychain");
                Ok(None)
            }
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.entry(account)?
            .set_password(secret)
            .map_err(|e| keychain_unavailable(&e))
    }

    fn delete(&self, account: &str) -> Result<()> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_unavailable(&e)),
        }
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryStore {
    secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self
            .secrets
            .lock()
            .map_err(|_| CoreError::Invalid("secret store lock poisoned".into()))?
            .get(account)
            .cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.secrets
            .lock()
            .map_err(|_| CoreError::Invalid("secret store lock poisoned".into()))?
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.secrets
            .lock()
            .map_err(|_| CoreError::Invalid("secret store lock poisoned".into()))?
            .remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::default();
        assert_eq!(s.get("a").unwrap(), None);
        s.set("a", "k1").unwrap();
        s.set("a", "k2").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("k2"));
        s.delete("a").unwrap();
        s.delete("a").unwrap(); // removing a missing key is fine
        assert_eq!(s.get("a").unwrap(), None);
    }

    /// Touches the real OS keychain (and may show an unlock dialog), so it only runs on request:
    /// `cargo test -p cairn-core -- --ignored os_keychain`.
    #[test]
    #[ignore = "uses the real system keychain"]
    fn os_keychain_round_trip() {
        let store = OsKeychain::new("cairn-test-throwaway");
        let account = "round-trip";
        assert_eq!(store.get(account).unwrap(), None);
        store.set(account, "secret-1").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("secret-1"));
        store.set(account, "secret-2").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("secret-2"));
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
        store.delete(account).unwrap();
    }
}
