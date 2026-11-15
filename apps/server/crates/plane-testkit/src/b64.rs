// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two base64 dialects the harness has to reproduce.
//!
//! Django's signing code uses `urlsafe_b64encode` with every trailing `=`
//! stripped (`py:django/core/signing.py:89-95`), while the PBKDF2 password
//! hasher uses the standard alphabet *with* padding
//! (`py:django/contrib/auth/hashers.py:329-334`). The harness keeps its own
//! encoder instead of pulling in a base64 crate: both are a dozen lines and the
//! exact alphabet and padding behaviour is part of what is being tested.

const URLSAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_with(alphabet: &[u8; 64], bytes: &[u8], pad: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(alphabet[((triple >> 18) & 0x3f) as usize] as char);
        out.push(alphabet[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(alphabet[((triple >> 6) & 0x3f) as usize] as char);
        } else if pad {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(alphabet[(triple & 0x3f) as usize] as char);
        } else if pad {
            out.push('=');
        }
    }
    out
}

fn decode_with(alphabet: &[u8; 64], text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
    let mut accumulator: u32 = 0;
    let mut bits = 0_u32;
    for byte in trimmed.bytes() {
        let value = alphabet.iter().position(|allowed| *allowed == byte)? as u32;
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// `urlsafe_b64encode(...).strip(b"=")`, as Django's `b64_encode` does.
pub fn encode_urlsafe_unpadded(bytes: &[u8]) -> String {
    encode_with(URLSAFE, bytes, false)
}

/// Inverse of [`encode_urlsafe_unpadded`], tolerating missing padding.
pub fn decode_urlsafe_unpadded(text: &str) -> Option<Vec<u8>> {
    decode_with(URLSAFE, text)
}

/// `base64.b64encode(...)` with padding, as Django's password hasher does.
pub fn encode_standard(bytes: &[u8]) -> String {
    encode_with(STANDARD, bytes, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlsafe_matches_python() {
        // Python: base64.urlsafe_b64encode(b"\xfb\xff").rstrip(b"=") == b"-_8"
        assert_eq!(encode_urlsafe_unpadded(&[0xfb, 0xff]), "-_8");
        assert_eq!(decode_urlsafe_unpadded("-_8"), Some(vec![0xfb, 0xff]));
        // A 32-byte digest is 43 characters without padding.
        let digest: Vec<u8> = (0..32).collect();
        let encoded = encode_urlsafe_unpadded(&digest);
        assert_eq!(encoded.len(), 43);
        assert!(!encoded.ends_with('='));
        assert_eq!(decode_urlsafe_unpadded(&encoded), Some(digest));
    }

    #[test]
    fn standard_matches_python() {
        // Python: base64.b64encode(b"abc") == b"YWJj"
        assert_eq!(encode_standard(b"abc"), "YWJj");
        // A 32-byte digest is 44 characters including one padding character.
        let digest = [0x42_u8; 32];
        let encoded = encode_standard(&digest);
        assert_eq!(encoded.len(), 44);
        assert!(encoded.ends_with('='));
    }

    #[test]
    fn invalid_characters_are_rejected() {
        assert_eq!(decode_urlsafe_unpadded("a b"), None);
        assert_eq!(decode_urlsafe_unpadded("a+b"), None);
        assert_eq!(decode_urlsafe_unpadded(""), Some(Vec::new()));
    }
}
