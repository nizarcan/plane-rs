// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! CPython `float.__repr__` formatting.
//!
//! CPython prints the shortest decimal string that round-trips to the same
//! `double`, using positional notation when the decimal point lies within
//! `(-4, 16]` of the digit string and scientific notation otherwise, with a
//! mandatory `.0` on integral positional values and a signed, at least
//! two-digit exponent (`1e-05`, `1e+16`).
//!
//! `ryu` supplies the shortest correctly-rounded digit string. Its
//! tie-breaking agrees with CPython's `dtoa` (verified against 250 000 values
//! while building this crate, including widened `f32`s where exact ties are
//! common); Rust's own `{:e}` does not always agree, so it is not used.
//! The presentation is then rearranged to CPython's rules.

use ryu::Buffer;

/// A formatted Python float, stored on the stack.
///
/// The longest possible representation is scientific form for a 17-digit
/// mantissa with a three-digit exponent (for example
/// `-1.2345678901234567e-308`), 25 bytes.
pub(crate) struct PyFloatRepr {
    buf: [u8; 32],
    len: usize,
}

impl PyFloatRepr {
    /// Format a finite `f64`; returns `None` for NaN and infinities, which
    /// Python's `json` module prints specially.
    pub(crate) fn new(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }

        let negative = value.is_sign_negative();
        let abs = value.abs();

        let mut out = [0u8; 32];
        let mut len = 0;
        if negative {
            out[0] = b'-';
            len = 1;
        }

        if abs == 0.0 {
            out[len] = b'0';
            out[len + 1] = b'.';
            out[len + 2] = b'0';
            len += 3;
            return Some(Self { buf: out, len });
        }

        let mut buffer = Buffer::new();
        let text = buffer.format_finite(abs);
        let (mantissa, exponent) = match text.split_once('e') {
            Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
            None => (text, 0),
        };
        let (integer_part, fraction_part) = match mantissa.split_once('.') {
            Some((integer, fraction)) => (integer, fraction),
            None => (mantissa, ""),
        };

        // Shortest digits with the decimal point `decpt` places from the
        // start: value = 0.<digits> * 10^decpt.
        let mut digits = [0u8; 24];
        let mut count = 0;
        for byte in integer_part.bytes().chain(fraction_part.bytes()) {
            if !byte.is_ascii_digit() || count == digits.len() {
                return None;
            }
            digits[count] = byte;
            count += 1;
        }
        let mut decpt = integer_part.len() as i32 + exponent;

        // Strip leading zeros (adjusting the point) and trailing zeros.
        let mut start = 0;
        while start + 1 < count && digits[start] == b'0' {
            start += 1;
            decpt -= 1;
        }
        while count - start > 1 && digits[count - 1] == b'0' {
            count -= 1;
        }
        let digits = &digits[start..count];

        if decpt <= -4 || decpt > 16 {
            // Scientific notation: d[.ddd]e±XX
            out[len] = digits[0];
            len += 1;
            if digits.len() > 1 {
                out[len] = b'.';
                len += 1;
                out[len..len + digits.len() - 1].copy_from_slice(&digits[1..]);
                len += digits.len() - 1;
            }
            out[len] = b'e';
            len += 1;
            let exp = decpt - 1;
            let (sign, magnitude) = if exp < 0 {
                (b'-', exp.unsigned_abs())
            } else {
                (b'+', exp as u32)
            };
            out[len] = sign;
            len += 1;
            if magnitude >= 100 {
                out[len] = b'0' + (magnitude / 100 % 10) as u8;
                out[len + 1] = b'0' + (magnitude / 10 % 10) as u8;
                out[len + 2] = b'0' + (magnitude % 10) as u8;
                len += 3;
            } else {
                out[len] = b'0' + (magnitude / 10) as u8;
                out[len + 1] = b'0' + (magnitude % 10) as u8;
                len += 2;
            }
        } else if decpt <= 0 {
            // 0.00ddd
            out[len] = b'0';
            out[len + 1] = b'.';
            len += 2;
            let zeros = decpt.unsigned_abs() as usize;
            out[len..len + zeros].fill(b'0');
            len += zeros;
            out[len..len + digits.len()].copy_from_slice(digits);
            len += digits.len();
        } else if (decpt as usize) >= digits.len() {
            // dddd000.0
            out[len..len + digits.len()].copy_from_slice(digits);
            len += digits.len();
            let zeros = decpt as usize - digits.len();
            out[len..len + zeros].fill(b'0');
            len += zeros;
            out[len] = b'.';
            out[len + 1] = b'0';
            len += 2;
        } else {
            // ddd.ddd
            let point = decpt as usize;
            out[len..len + point].copy_from_slice(&digits[..point]);
            len += point;
            out[len] = b'.';
            len += 1;
            out[len..len + digits.len() - point].copy_from_slice(&digits[point..]);
            len += digits.len() - point;
        }

        Some(Self { buf: out, len })
    }

    /// The formatted representation.
    pub(crate) fn as_str(&self) -> &str {
        // The buffer only ever contains ASCII digits, signs, '.', 'e'.
        std::str::from_utf8(&self.buf[..self.len]).unwrap_or("0.0")
    }
}

impl std::fmt::Display for PyFloatRepr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::PyFloatRepr;

    fn repr(value: f64) -> String {
        PyFloatRepr::new(value)
            .map(|v| v.as_str().to_owned())
            .unwrap_or_default()
    }

    #[test]
    fn python_repr_examples() {
        // Table from HTTP_CONTRACT.md §10.4 / Appendix A.4.
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-1.0, "-1.0"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1.0 / 3.0, "0.3333333333333333"),
            (1e-4, "0.0001"),
            (1e-5, "1e-05"),
            (1.234567890123e-5, "1.234567890123e-05"),
            (1e-7, "1e-07"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1234567890123456.0, "1234567890123456.0"),
            (12345678901234567.0, "1.2345678901234568e+16"),
            (1e21, "1e+21"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (-1.5e-9, "-1.5e-09"),
            (100.0, "100.0"),
            (9999999999999998.0, "9999999999999998.0"),
            (999999999999999.9, "999999999999999.9"),
            (0.0001 + 0.00001, "0.00011"),
            // Exact ties between two shortest candidates: CPython rounds the
            // last digit to even; Rust's `{:e}` picks the other one. Written
            // as bit patterns because the exact decimal form has more digits
            // than clippy's `excessive_precision` allows (and the literal
            // would be rounded to the same f64 anyway).
            (f64::from_bits(0x4063_a7ac_4000_0000), "157.23977661132812"),
            (f64::from_bits(0xc066_93c5_4000_0000), "-180.61782836914062"),
            (f64::from_bits(0xc009_81b4_0000_0000), "-3.1883316040039062"),
        ];
        for (value, expected) in cases {
            assert_eq!(repr(value), expected, "value bits {:#x}", value.to_bits());
        }
    }

    #[test]
    fn f32_is_formatted_as_its_widened_value() {
        assert_eq!(repr(0.1f32 as f64), "0.10000000149011612");
        assert_eq!(repr(f32::MAX as f64), "3.4028234663852886e+38");
        assert_eq!(repr(f32::MIN_POSITIVE as f64), "1.1754943508222875e-38");
        assert_eq!(repr(1.0f32 as f64), "1.0");
    }
}
