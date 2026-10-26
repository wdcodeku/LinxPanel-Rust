//! Time-based one-time passwords (RFC 6238) for two-factor login, compatible
//! with Google Authenticator, Microsoft Authenticator, 1Password and the like.

use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

const STEP_SECONDS: u64 = 30;
const DIGITS: u32 = 6;
/// Accept the previous and next code too, to absorb clock drift on the phone.
const WINDOW: i64 = 1;
const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// A new random 160-bit secret, base32-encoded for authenticator apps.
pub fn generate_secret() -> Result<String, String> {
    let mut bytes = [0u8; 20];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "System random number generator unavailable".to_string())?;
    Ok(base32_encode(&bytes))
}

pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Decodes base32, ignoring case, spaces and padding as authenticator apps show them.
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let value = BASE32
            .iter()
            .position(|&b| b as char == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

fn code_at(secret: &[u8], step: u64) -> u32 {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let digest = hmac::sign(&key, &step.to_be_bytes());
    let digest = digest.as_ref();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    value % 10u32.pow(DIGITS)
}

fn current_step() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64 / STEP_SECONDS
}

/// Checks `code` against `secret` around the current time. Returns the matched
/// time step, which must be newer than `last_step` so a code cannot be replayed.
pub fn verify(secret: &str, code: &str, last_step: i64) -> Option<i64> {
    verify_at(secret, code, last_step, current_step())
}

fn verify_at(secret: &str, code: &str, last_step: i64, now: u64) -> Option<i64> {
    let code = code.trim().replace(' ', "");
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let expected: u32 = code.parse().ok()?;
    let secret = base32_decode(secret)?;
    (-WINDOW..=WINDOW)
        .filter_map(|delta| now.checked_add_signed(delta))
        .filter(|step| *step as i64 > last_step)
        .find(|step| constant_time_eq(code_at(&secret, *step), expected))
        .map(|step| step as i64)
}

/// Compares without an early exit, so timing does not reveal matching digits.
fn constant_time_eq(a: u32, b: u32) -> bool {
    a.to_be_bytes()
        .iter()
        .zip(b.to_be_bytes())
        .fold(0u8, |diff, (x, y)| diff | (x ^ y))
        == 0
}

/// The `otpauth://` URI an authenticator app scans from a QR code.
pub fn otpauth_uri(secret: &str, account: &str, issuer: &str) -> String {
    format!(
        "otpauth://totp/{issuer}:{account}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
        issuer = percent_encode(issuer),
        account = percent_encode(account),
    )
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{:02X}", b)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_test_vector() {
        // RFC 6238 appendix B, SHA-1, T = 59s -> 94287082 (8 digits) -> 287082.
        let secret = b"12345678901234567890";
        assert_eq!(code_at(secret, 59 / STEP_SECONDS), 287082);
        // T = 1111111109 -> 07081804 -> 081804.
        assert_eq!(code_at(secret, 1111111109 / STEP_SECONDS), 81804);
    }

    #[test]
    fn base32_roundtrip() {
        let secret = generate_secret().unwrap();
        assert_eq!(secret.len(), 32);
        let bytes = base32_decode(&secret).unwrap();
        assert_eq!(bytes.len(), 20);
        assert_eq!(base32_encode(&bytes), secret);
        assert_eq!(
            base32_encode(b"12345678901234567890"),
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        );
        assert_eq!(base32_decode("gezd gnbv").unwrap(), b"12345");
        assert!(base32_decode("not base32!").is_none());
    }

    #[test]
    fn verify_window_and_replay() {
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let raw = b"12345678901234567890";
        let now = 1_000_000u64;
        let code = format!("{:06}", code_at(raw, now));
        assert_eq!(verify_at(secret, &code, 0, now), Some(now as i64));
        // Previous step still accepted (clock drift), two steps back is not.
        let previous = format!("{:06}", code_at(raw, now - 1));
        assert_eq!(verify_at(secret, &previous, 0, now), Some(now as i64 - 1));
        let stale = format!("{:06}", code_at(raw, now - 2));
        assert_eq!(verify_at(secret, &stale, 0, now), None);
        // A code already used cannot be used again.
        assert_eq!(verify_at(secret, &code, now as i64, now), None);
        assert_eq!(verify_at(secret, "12345", 0, now), None);
        assert_eq!(verify_at(secret, "abcdef", 0, now), None);
    }

    #[test]
    fn otpauth_uri_is_encoded() {
        let uri = otpauth_uri("ABC", "admin user", "LinxPanel");
        assert!(
            uri.starts_with("otpauth://totp/LinxPanel:admin%20user?secret=ABC&issuer=LinxPanel")
        );
    }
}
