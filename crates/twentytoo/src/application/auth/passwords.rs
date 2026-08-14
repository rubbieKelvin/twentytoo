//! Password hashing with Argon2id.
//!
//! Passwords are hashed with the [`Argon2`] default (Argon2id) and a fresh
//! random salt on every call; the stored value is the full PHC string.
//! Verification returns `false` on any failure — wrong password, malformed
//! hash, or an internal error — so callers never distinguish those cases
//! (which would leak account state).

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

/// Hash `password` into an argon2 PHC string with a fresh random salt.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    return Ok(hash);
}

/// Whether `password` matches the stored `hash`. `false` on any error,
/// including a malformed or empty hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    return Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert_ne!(hash, "correct horse battery staple");
        assert!(verify_password("correct horse battery staple", &hash));
    }

    #[test]
    fn wrong_password_fails() {
        let hash = hash_password("right-password").expect("hash");
        assert!(!verify_password("wrong-password", &hash));
    }

    #[test]
    fn malformed_hash_fails() {
        assert!(!verify_password("anything", "not-a-phc-string"));
        assert!(!verify_password("anything", ""));
    }
}
