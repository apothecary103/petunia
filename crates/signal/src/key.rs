//! The passphrase the store is encrypted with.
//!
//! It lives where the platform keeps secrets -- the Keychain on macOS, the
//! Secret Service on Linux, which is what both GNOME Keyring and KWallet are --
//! and nowhere else. Not in `config.toml`: a key beside the file it unlocks is
//! an inventory of what was taken, not a lock.
//!
//! Generated once, on the launch that finds none, and never shown. There is no
//! way to type it in and no way to read it out, because there is nothing a
//! person could usefully do with sixty-four characters of hex -- and offering to
//! export it would make the Keychain one of two places it lives.
//!
//! Failing to reach the keyring is an error rather than a fallback to an
//! unencrypted store. A client that quietly stops encrypting when the lock is
//! awkward has a setting that describes a wish.

use rand::RngCore;

/// What the entry is called where the platform lists it, which is somewhere a
/// person may well look -- so it says which application and which secret.
const SERVICE: &str = "petunia";
const ACCOUNT: &str = "store-encryption-key";

/// The passphrase for this account's store, generating and keeping one the first
/// time. Hex rather than raw bytes: it goes into a `PRAGMA key` as a string
/// literal, and every keyring backend on both platforms stores text.
///
/// The keyring's own error rather than the crate's: this reaches one thing and
/// can fail in one way, and callers already convert.
pub fn passphrase() -> Result<String, keyring::Error> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)?;

    match entry.get_password() {
        Ok(existing) => Ok(existing),
        Err(keyring::Error::NoEntry) => {
            let fresh = generate();
            entry.set_password(&fresh)?;
            tracing::info!("generated the store encryption key");
            Ok(fresh)
        }
        Err(error) => Err(error),
    }
}

fn generate() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    petunia_data::hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_key_is_thirty_two_bytes_of_hex() {
        let key = generate();

        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// The whole value of generating one rather than deriving it from anything.
    #[test]
    fn two_generated_keys_differ() {
        assert_ne!(generate(), generate());
    }
}
