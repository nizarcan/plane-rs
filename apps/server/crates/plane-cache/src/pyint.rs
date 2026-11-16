// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A Python `int`, stored as a sign and a little-endian magnitude.
//!
//! The pickle codec needs the binary two's-complement form CPython's
//! `encode_long` writes (`Modules/_pickle.c`, `Lib/pickle.py:encode_long`), the
//! decoder needs to read it back, and django-redis' `int(value)` fallback in
//! `DefaultClient.decode` accepts an unbounded number of digits. Decimal
//! digits therefore have to be converted in both directions; there is no
//! `num-bigint` dependency because these are the only operations needed.
//!
//! Only non-negative magnitudes are stored (`-0` is normalised to `0`), and
//! the magnitude never has a trailing zero byte, so equal values compare equal.

use std::fmt;

/// A Python `int` of unbounded size.
///
/// Zero is `negative == false` with an empty magnitude. That is the only
/// representation of zero.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PyInt {
    /// Sign; `false` for zero and positive values.
    negative: bool,
    /// `|value|` little-endian, with no trailing zero bytes.
    magnitude: Vec<u8>,
}

impl PyInt {
    /// `0`.
    #[must_use]
    pub fn zero() -> Self {
        Self::default()
    }

    /// `i64` → `int`.
    #[must_use]
    pub fn from_i64(value: i64) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let negative = value < 0;
        let magnitude = value.unsigned_abs().to_le_bytes().to_vec();
        let mut out = Self {
            negative,
            magnitude,
        };
        out.trim();
        out
    }

    /// `u64` → `int`.
    #[must_use]
    pub fn from_u64(value: u64) -> Self {
        let mut out = Self {
            negative: false,
            magnitude: value.to_le_bytes().to_vec(),
        };
        out.trim();
        out
    }

    /// `u128` → `int` (a UUID's `int`).
    #[must_use]
    pub fn from_u128(value: u128) -> Self {
        let mut out = Self {
            negative: false,
            magnitude: value.to_le_bytes().to_vec(),
        };
        out.trim();
        out
    }

    /// Build from a sign and a big-endian magnitude (as `int.from_bytes`
    /// reads it).
    #[must_use]
    pub fn from_be_magnitude(negative: bool, big_endian: &[u8]) -> Self {
        let mut magnitude: Vec<u8> = big_endian.iter().rev().copied().collect();
        let mut out = Self {
            negative,
            magnitude: std::mem::take(&mut magnitude),
        };
        out.trim();
        out
    }

    fn trim(&mut self) {
        while self.magnitude.last() == Some(&0) {
            self.magnitude.pop();
        }
        if self.magnitude.is_empty() {
            self.negative = false;
        }
    }

    /// Whether the value is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.magnitude.is_empty()
    }

    /// Whether the value is negative (zero is not).
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.negative
    }

    /// The `i64` value, when it fits.
    #[must_use]
    pub fn to_i64(&self) -> Option<i64> {
        if self.magnitude.len() > 8 {
            return None;
        }
        let mut magnitude = [0_u8; 8];
        magnitude[..self.magnitude.len()].copy_from_slice(&self.magnitude);
        let value = u64::from_le_bytes(magnitude);
        if self.negative {
            // `-2**63` is representable; anything larger in magnitude is not.
            if value > 1 << 63 {
                return None;
            }
            Some((value as i64).wrapping_neg())
        } else {
            i64::try_from(value).ok()
        }
    }

    /// The `u64` value, when it fits.
    #[must_use]
    pub fn to_u64(&self) -> Option<u64> {
        if self.negative || self.magnitude.len() > 8 {
            return None;
        }
        let mut magnitude = [0_u8; 8];
        magnitude[..self.magnitude.len()].copy_from_slice(&self.magnitude);
        Some(u64::from_le_bytes(magnitude))
    }

    /// The `u128` value, when it fits and is not negative.
    #[must_use]
    pub fn to_u128(&self) -> Option<u128> {
        if self.negative || self.magnitude.len() > 16 {
            return None;
        }
        let mut magnitude = [0_u8; 16];
        magnitude[..self.magnitude.len()].copy_from_slice(&self.magnitude);
        Some(u128::from_le_bytes(magnitude))
    }

    /// The magnitude as 16 big-endian bytes, for `uuid.UUID(int=…)`.
    #[must_use]
    pub fn to_be_bytes_16(&self) -> Option<[u8; 16]> {
        let value = self.to_u128()?;
        Some(value.to_be_bytes())
    }

    /// `int(text)` with CPython's base-10 rules: surrounding ASCII whitespace
    /// is ignored, a leading `+`/`-` is allowed, and single underscores may
    /// separate digits.
    ///
    /// Returns `None` where CPython raises `ValueError`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim_matches(|ch: char| {
            matches!(
                ch,
                ' ' | '\t'
                    | '\n'
                    | '\r'
                    | '\u{0b}'
                    | '\u{0c}'
                    | '\u{1c}'
                    | '\u{1d}'
                    | '\u{1e}'
                    | '\u{1f}'
            )
        });
        let (negative, digits) = match trimmed.as_bytes().first() {
            Some(b'-') => (true, &trimmed[1..]),
            Some(b'+') => (false, &trimmed[1..]),
            _ => (false, trimmed),
        };
        if digits.is_empty() {
            return None;
        }
        let mut out = Self::zero();
        let mut previous_underscore = true; // an underscore may not lead
        for byte in digits.bytes() {
            match byte {
                b'0'..=b'9' => {
                    out.mul_add_small(10, byte - b'0');
                    previous_underscore = false;
                }
                b'_' => {
                    if previous_underscore {
                        return None;
                    }
                    previous_underscore = true;
                }
                _ => return None,
            }
        }
        if previous_underscore {
            return None;
        }
        out.negative = negative;
        out.trim();
        Some(out)
    }

    /// `out = out * factor + addend` on the little-endian magnitude.
    fn mul_add_small(&mut self, factor: u32, addend: u8) {
        let mut carry = u64::from(addend);
        for byte in &mut self.magnitude {
            let value = u64::from(*byte) * u64::from(factor) + carry;
            *byte = (value & 0xff) as u8;
            carry = value >> 8;
        }
        while carry > 0 {
            self.magnitude.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }

    /// Divide the magnitude by `divisor`, returning the remainder.
    fn div_small(&mut self, divisor: u32) -> u32 {
        let mut remainder = 0_u64;
        for byte in self.magnitude.iter_mut().rev() {
            let value = (remainder << 8) | u64::from(*byte);
            *byte = (value / u64::from(divisor)) as u8;
            remainder = value % u64::from(divisor);
        }
        self.trim();
        remainder as u32
    }

    /// `str(int)`: the decimal representation, with a leading `-` when
    /// negative. Applies no `sys.int_max_str_digits` limit; the callers that
    /// can hit it (django-redis' `int(value)`) only ever parse short strings.
    #[must_use]
    pub fn to_decimal_string(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        // Nine digits at a time: 10**9 is the largest power of ten in a u32,
        // so one `div_small` produces nine decimal digits.
        let mut chunks: Vec<u32> = Vec::with_capacity(self.magnitude.len() / 4 + 1);
        let mut copy = Self {
            negative: false,
            magnitude: self.magnitude.clone(),
        };
        while !copy.is_zero() {
            chunks.push(copy.div_small(1_000_000_000));
        }
        let mut out = String::with_capacity(self.magnitude.len() * 3 + 2);
        if self.negative {
            out.push('-');
        }
        // The most significant chunk is not zero-padded; the rest are.
        let Some((first, rest)) = chunks.split_last() else {
            return "0".to_string();
        };
        out.push_str(&first.to_string());
        for chunk in rest.iter().rev() {
            use std::fmt::Write as _;
            let _ = write!(out, "{chunk:09}");
        }
        out
    }

    /// CPython's `encode_long` (`Lib/pickle.py`): two's-complement
    /// little-endian bytes, minimal length, `b""` for zero.
    #[must_use]
    pub fn to_twos_complement(&self) -> Vec<u8> {
        if self.is_zero() {
            return Vec::new();
        }
        if !self.negative {
            let mut out = self.magnitude.clone();
            if out.last().is_some_and(|byte| byte & 0x80 != 0) {
                out.push(0);
            }
            return out;
        }
        // Negative: CPython computes `nbytes = bit_length // 8 + 1` bytes of
        // two's complement and then drops a redundant `0xff` top byte
        // (`Lib/pickle.py:encode_long`).
        let magnitude = &self.magnitude;
        let top = *magnitude.last().unwrap_or(&0);
        let bit_length = (magnitude.len() - 1) * 8 + (8 - top.leading_zeros() as usize);
        let mut out = vec![0_u8; bit_length / 8 + 1];
        let copy = magnitude.len().min(out.len());
        out[..copy].copy_from_slice(&magnitude[..copy]);
        let mut carry = true;
        for byte in &mut out {
            *byte = !*byte;
            if carry {
                let (value, overflow) = byte.overflowing_add(1);
                *byte = value;
                carry = overflow;
            }
        }
        if out.len() > 1 && out[out.len() - 1] == 0xff && out[out.len() - 2] & 0x80 != 0 {
            out.pop();
        }
        out
    }

    /// `decode_long`: two's-complement little-endian bytes → `int`.
    #[must_use]
    pub fn from_twos_complement(bytes: &[u8]) -> Self {
        let Some(last) = bytes.last() else {
            return Self::zero();
        };
        let negative = last & 0x80 != 0;
        if !negative {
            return Self::from_be_magnitude(
                false,
                &bytes.iter().rev().copied().collect::<Vec<_>>(),
            );
        }
        // Negative: invert and add one to get the magnitude.
        let mut magnitude: Vec<u8> = bytes.iter().map(|byte| !*byte).collect();
        let mut carry = true;
        for byte in &mut magnitude {
            if !carry {
                break;
            }
            let (value, overflow) = byte.overflowing_add(1);
            *byte = value;
            carry = overflow;
        }
        let mut out = Self {
            negative: true,
            magnitude,
        };
        out.trim();
        out
    }
}

impl fmt::Display for PyInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal_string())
    }
}

impl From<i64> for PyInt {
    fn from(value: i64) -> Self {
        Self::from_i64(value)
    }
}

impl From<u128> for PyInt {
    fn from(value: u128) -> Self {
        Self::from_u128(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> (String, String) {
        let value = PyInt::parse(text).expect("parses");
        let bytes = value.to_twos_complement();
        let back = PyInt::from_twos_complement(&bytes);
        (value.to_decimal_string(), back.to_decimal_string())
    }

    /// `encode_long` / `decode_long` pairs, from Python 3.12:
    /// `binascii.hexlify(encode_long(n))`.
    #[test]
    fn encode_long_matches_cpython() {
        let cases: &[(&str, &str)] = &[
            ("0", ""),
            ("1", "01"),
            ("127", "7f"),
            ("128", "8000"),
            ("255", "ff00"),
            ("256", "0001"),
            ("32767", "ff7f"),
            ("32768", "008000"),
            ("65535", "ffff00"),
            ("65536", "000001"),
            ("8388607", "ffff7f"),
            ("8388608", "00008000"),
            ("-1", "ff"),
            ("-2", "fe"),
            ("-127", "81"),
            ("-128", "80"),
            ("-129", "7fff"),
            ("-255", "01ff"),
            ("-256", "00ff"),
            ("-257", "fffe"),
            ("-32768", "0080"),
            ("-32769", "ff7fff"),
            ("2147483647", "ffffff7f"),
            ("2147483648", "0000008000"),
            ("4294967295", "ffffffff00"),
            ("4294967296", "0000000001"),
            ("-2147483648", "00000080"),
            ("-2147483649", "ffffff7fff"),
            ("-4294967296", "00000000ff"),
            (
                "1267650600228229401496703205376",
                "00000000000000000000000010",
            ),
            (
                "-1267650600228229401496703205376",
                "000000000000000000000000f0",
            ),
        ];
        for (decimal, hex) in cases {
            let value = PyInt::parse(decimal).expect("parses");
            let encoded = value.to_twos_complement();
            assert_eq!(crate::hex(&encoded), *hex, "encode_long({decimal})");
            assert_eq!(
                PyInt::from_twos_complement(&encoded).to_decimal_string(),
                *decimal,
                "decode_long({decimal})"
            );
        }
    }

    #[test]
    fn decimal_round_trips() {
        for text in [
            "0",
            "1",
            "-1",
            "123456789012345678901234567890",
            "-123456789012345678901234567890",
            "999999999999999999999999999999999999999999999999",
            "1000000000",
            "-1000000000",
            "4294967295",
            "18446744073709551615",
            "9223372036854775807",
            "-9223372036854775808",
        ] {
            assert_eq!(round_trip(text), (text.to_string(), text.to_string()));
        }
    }

    #[test]
    fn parse_matches_python_int() {
        assert_eq!(PyInt::parse("  42  ").unwrap().to_decimal_string(), "42");
        assert_eq!(PyInt::parse("+7").unwrap().to_decimal_string(), "7");
        assert_eq!(PyInt::parse("-0").unwrap().to_decimal_string(), "0");
        assert_eq!(PyInt::parse("1_0").unwrap().to_decimal_string(), "10");
        assert!(PyInt::parse("").is_none());
        assert!(PyInt::parse("-").is_none());
        assert!(PyInt::parse("1_").is_none());
        assert!(PyInt::parse("_1").is_none());
        assert!(PyInt::parse("1__0").is_none());
        assert!(PyInt::parse("0x10").is_none());
        assert!(PyInt::parse("1.0").is_none());
        assert!(PyInt::parse("\u{0661}").is_none(), "Arabic-Indic digit");
    }

    #[test]
    fn fits_match_rust_ranges() {
        assert_eq!(PyInt::from_i64(0).to_u64(), Some(0));
        assert_eq!(PyInt::from_i64(-1).to_u64(), None);
        assert_eq!(PyInt::from_i64(-1).to_i64(), Some(-1));
        assert_eq!(PyInt::from_i64(i64::MIN).to_i64(), Some(i64::MIN));
        assert_eq!(PyInt::from_u64(i64::MAX as u64 + 1).to_i64(), None);
        assert_eq!(PyInt::from_u128(u128::MAX).to_u128(), Some(u128::MAX));
        assert_eq!(
            PyInt::from_u128(u128::MAX).to_be_bytes_16(),
            Some([0xff; 16])
        );
    }

    #[test]
    fn uuid_sized_values_need_a_sign_byte() {
        // A UUID whose top bit is set has a 17-byte `encode_long`.
        let value = PyInt::from_u128(u128::MAX);
        assert_eq!(value.to_twos_complement().len(), 17);
        let value = PyInt::from_u128(1);
        assert_eq!(crate::hex(&value.to_twos_complement()), "01");
    }
}
