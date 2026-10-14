//! The cleanup server API key, kept in the macOS Keychain.
//!
//! The key is stored as a generic Keychain password, so it never lands in
//! `settings.json`. Saving an empty key deletes it.

use anyhow::{Context, Result};
use keyring::Entry;

pub const KEYCHAIN_SERVICE: &str = "com.softmaxe.whisper";
pub const CLEANUP_API_KEY_ACCOUNT: &str = "cleanup-custom-api-key";

fn cleanup_entry() -> Result<Entry> {
    Entry::new(KEYCHAIN_SERVICE, CLEANUP_API_KEY_ACCOUNT).context("opening Keychain entry")
}

/// The stored key, or `None` when none is saved.
pub fn get_cleanup_api_key() -> Result<Option<String>> {
    read(&cleanup_entry()?)
}

/// Saves the key verbatim; an empty key deletes it.
pub fn set_cleanup_api_key(key: &str) -> Result<()> {
    write(&cleanup_entry()?, key)
}

pub fn delete_cleanup_api_key() -> Result<()> {
    remove(&cleanup_entry()?)
}

fn read(entry: &Entry) -> Result<Option<String>> {
    match entry.get_password() {
        Ok(key) if key.is_empty() => Ok(None),
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err).context("reading cleanup API key from Keychain"),
    }
}

fn write(entry: &Entry, key: &str) -> Result<()> {
    if key.is_empty() {
        return remove(entry);
    }
    entry
        .set_password(key)
        .context("saving cleanup API key to Keychain")
}

fn remove(entry: &Entry) -> Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err).context("deleting cleanup API key from Keychain"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring::mock::MockCredential;

    /// An in-memory entry, so tests never touch the real Keychain.
    fn mock_entry() -> Entry {
        Entry::new_with_credential(Box::new(MockCredential::default()))
    }

    #[test]
    fn round_trips_and_deletes_a_key() {
        let entry = mock_entry();
        assert_eq!(read(&entry).unwrap(), None);
        write(&entry, "sk-test").unwrap();
        assert_eq!(read(&entry).unwrap().as_deref(), Some("sk-test"));
        remove(&entry).unwrap();
        assert_eq!(read(&entry).unwrap(), None);
        remove(&entry).unwrap();
    }

    #[test]
    fn saving_an_empty_key_deletes_it() {
        let entry = mock_entry();
        write(&entry, "sk-test").unwrap();
        write(&entry, "").unwrap();
        assert_eq!(read(&entry).unwrap(), None);
    }
}
