//! Opaque step tokens and one-time login codes.
//!
//! A step token is 32 random bytes; only its SHA-256 hex hash is stored
//! (the same convention as the sessions table). A login code is a 6-digit
//! number emailed to the user; like the token, only its hash is stored —
//! codes are single-use and attempt-capped, so a plain hash suffices.

use rand::Rng;
use sha2::{Digest, Sha256};

/// A freshly minted login step token: 32 random bytes as 64 lowercase hex
/// characters.
pub fn new_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    return hex(&bytes);
}

/// The SHA-256 hex hash of a step token (what the database stores).
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    return hex(&hasher.finalize());
}

/// A fresh one-time login code: 6 decimal digits, zero-padded.
pub fn new_code() -> String {
    let code: u32 = rand::rng().random_range(0..1_000_000);
    return format!("{code:06}");
}

/// The SHA-256 hex hash of a login code.
pub fn hash_code(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    return hex(&hasher.finalize());
}

/// Lowercase hex encoding of `bytes` (no external `hex` dependency).
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    return out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_is_64_lowercase_hex_and_unique() {
        let a = new_token();
        let b = new_token();
        assert_eq!(a.len(), 64);
        assert_eq!(b.len(), 64);
        assert!(
            a.chars()
                .all(|c| return c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(a, b);
    }

    #[test]
    fn hash_token_is_stable_sha256_hex() {
        // SHA-256 of "abc", the standard test vector.
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(hash_token("abc"), expected);
    }

    #[test]
    fn new_code_is_six_digits_zero_padded() {
        for _ in 0..100 {
            let code = new_code();
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|c| return c.is_ascii_digit()));
        }
    }
}
