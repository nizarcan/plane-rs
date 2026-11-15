// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! An arbitrary-precision decimal that reproduces Python's `decimal.Decimal`
//! exactly for the operations Plane's serialisation paths perform:
//! construction from a string, `str()`, `format(value, "f")`, `float(value)`
//! and `quantize` (as DRF's `DecimalField` uses it).
//!
//! The representation is the one `Decimal.as_tuple()` exposes: sign, a
//! coefficient of decimal digits, and a power-of-ten exponent. The value is
//! `(-1)^sign * digits * 10^exponent`.

use std::fmt;
use std::str::FromStr;

/// An error constructing or quantising a [`PyDecimal`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PyDecimalError {
    /// The text is not a Python `Decimal` literal.
    #[error("invalid decimal literal: {0}")]
    Invalid(String),
    /// `quantize` would need more digits than the context precision allows;
    /// Python raises `decimal.InvalidOperation`.
    #[error("quantize result requires more than {0} digits")]
    InvalidOperation(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Finite(Finite),
    NaN { negative: bool, signaling: bool },
    Infinity { negative: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Finite {
    negative: bool,
    /// ASCII digits `b'0'..=b'9'`; never empty, and never with a leading
    /// zero unless the coefficient is exactly one zero digit.
    digits: Vec<u8>,
    exp: i64,
}

/// A Python `decimal.Decimal` value.
///
/// `str()`/`Display` reproduces Python's `to-scientific-string` convention:
/// plain notation when the exponent is non-positive and the adjusted
/// exponent is at least -6, scientific notation otherwise.
/// [`PyDecimal::to_fixed_string`] reproduces `f"{value:f}"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyDecimal(Kind);

impl PyDecimal {
    /// Parse a Python `Decimal` string literal (the form
    /// `Decimal(str(value).strip())` accepts in DRF's
    /// `DecimalField.to_representation`).
    pub fn parse(text: &str) -> Result<Self, PyDecimalError> {
        let text = text.trim();
        let (negative, rest) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };

        if rest.is_empty() {
            return Err(PyDecimalError::Invalid(text.to_owned()));
        }

        if rest.eq_ignore_ascii_case("inf") || rest.eq_ignore_ascii_case("infinity") {
            return Ok(Self(Kind::Infinity { negative }));
        }
        let lower = rest.to_ascii_lowercase();
        if lower == "nan"
            || lower == "snan"
            || is_nan_payload(&lower, "nan")
            || is_nan_payload(&lower, "snan")
        {
            return Ok(Self(Kind::NaN {
                negative,
                signaling: lower.starts_with('s'),
            }));
        }

        let (mantissa, exponent_text) = match rest.find(['e', 'E']) {
            Some(index) => (&rest[..index], Some(&rest[index + 1..])),
            None => (rest, None),
        };

        let mut exp: i64 = match exponent_text {
            Some(text) => {
                parse_exponent(text).ok_or_else(|| PyDecimalError::Invalid(text.to_owned()))?
            }
            None => 0,
        };

        let (integer_text, fraction_text) = match mantissa.split_once('.') {
            Some((integer, fraction)) => (integer, Some(fraction)),
            None => (mantissa, None),
        };

        let mut digits = parse_digit_run(integer_text)
            .ok_or_else(|| PyDecimalError::Invalid(integer_text.to_owned()))?;
        let fraction_digits = match fraction_text {
            Some(fraction) => {
                let parsed = parse_digit_run(fraction)
                    .ok_or_else(|| PyDecimalError::Invalid(fraction.to_owned()))?;
                exp -= parsed.len() as i64;
                parsed
            }
            None => Vec::new(),
        };

        if digits.is_empty() && fraction_digits.is_empty() {
            return Err(PyDecimalError::Invalid(text.to_owned()));
        }
        digits.extend_from_slice(&fraction_digits);

        // Decimal() strips leading zeros from the coefficient but keeps
        // trailing zeros as part of the coefficient (Decimal('1.2300') keeps
        // five digits with exponent -4).
        match digits.iter().position(|&digit| digit != b'0') {
            Some(first) => {
                digits.drain(..first);
            }
            None => digits = vec![b'0'],
        }

        Ok(Self(Kind::Finite(Finite {
            negative,
            digits,
            exp,
        })))
    }

    /// Whether the value is finite (Python `Decimal.is_finite()`).
    #[must_use]
    pub fn is_finite(&self) -> bool {
        matches!(self.0, Kind::Finite(_))
    }

    /// Whether the value is NaN (Python `Decimal.is_nan()`).
    #[must_use]
    pub fn is_nan(&self) -> bool {
        matches!(self.0, Kind::NaN { .. })
    }

    /// Whether the value is an infinity (Python `Decimal.is_infinite()`).
    #[must_use]
    pub fn is_infinite(&self) -> bool {
        matches!(self.0, Kind::Infinity { .. })
    }

    /// `str(value)`.
    #[must_use]
    pub fn to_python_string(&self) -> String {
        self.to_string()
    }

    /// `format(value, "f")`.
    #[must_use]
    pub fn to_fixed_string(&self) -> String {
        let mut out = String::new();
        // Writing to a String cannot fail.
        let _ = self.fmt_fixed(&mut out);
        out
    }

    /// A `Display` adapter for `format(value, "f")`, so serialisers can
    /// stream the digits without an intermediate `String`.
    pub(crate) fn as_fixed(&self) -> FixedDecimal<'_> {
        FixedDecimal(self)
    }

    /// `float(value)`: correctly rounded, or an infinity on overflow and
    /// `0.0` on underflow, exactly as CPython's `Decimal.__float__`.
    #[must_use]
    pub fn to_f64(&self) -> f64 {
        match &self.0 {
            Kind::NaN { .. } => f64::NAN,
            Kind::Infinity { negative } => {
                if *negative {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            }
            Kind::Finite(_) => {
                // `str(Decimal)` preserves every coefficient digit, so parsing
                // it is the same correctly-rounded conversion CPython does.
                // Extremely large exponents become infinities in both.
                self.to_string().parse::<f64>().unwrap_or(0.0)
            }
        }
    }

    /// `value.quantize(Decimal(".1") ** decimal_places, context=context)`
    /// with `context.prec = max_digits` and the context's default rounding
    /// (`ROUND_HALF_EVEN`), as DRF's `DecimalField.quantize` does.
    ///
    /// When `max_digits` is `None`, DRF copies the default context, whose
    /// precision is 28.
    pub fn quantize(
        &self,
        decimal_places: u32,
        max_digits: Option<u32>,
    ) -> Result<Self, PyDecimalError> {
        let precision = max_digits.unwrap_or(28) as usize;
        let target_exp = -i64::from(decimal_places);

        let finite = match &self.0 {
            Kind::Finite(finite) => finite,
            // quantize on NaN or Infinity returns the value unchanged.
            _ => return Ok(self.clone()),
        };

        let mut digits = finite.digits.clone();

        if finite.exp >= target_exp {
            let extra = i128::from(finite.exp) - i128::from(target_exp);
            let new_len = digits.len() as i128 + extra;
            if new_len > precision as i128 {
                return Err(PyDecimalError::InvalidOperation(max_digits.unwrap_or(28)));
            }
            let extra = extra.unsigned_abs() as usize;
            digits.resize(digits.len() + extra, b'0');
        } else {
            let drop = i128::from(target_exp) - i128::from(finite.exp);
            let len = digits.len() as i128;
            if drop >= len {
                // The whole coefficient is dropped. Only an exact half with
                // an even kept digit can round down; one more case rounds up.
                let round_up = if drop > len {
                    false
                } else {
                    let first = digits[0];
                    if first > b'5' {
                        true
                    } else if first < b'5' {
                        false
                    } else {
                        // Kept coefficient is zero (even): half rounds down,
                        // more than half rounds up.
                        digits[1..].iter().any(|&digit| digit != b'0')
                    }
                };
                digits = vec![if round_up { b'1' } else { b'0' }];
            } else {
                let keep = (len - drop) as usize;
                let first_dropped = digits[keep];
                let rest_nonzero = digits[keep + 1..].iter().any(|&digit| digit != b'0');
                let round_up = if first_dropped > b'5' {
                    true
                } else if first_dropped < b'5' {
                    false
                } else if rest_nonzero {
                    true
                } else {
                    let last_kept = if keep == 0 { b'0' } else { digits[keep - 1] };
                    (last_kept - b'0') % 2 == 1
                };
                digits.truncate(keep);
                if round_up {
                    let mut carry = true;
                    for index in (0..digits.len()).rev() {
                        if digits[index] < b'9' {
                            digits[index] += 1;
                            carry = false;
                            break;
                        }
                        digits[index] = b'0';
                    }
                    if carry {
                        digits.insert(0, b'1');
                    }
                }
                if digits.is_empty() {
                    digits.push(b'0');
                }
            }
            if digits.len() > precision {
                return Err(PyDecimalError::InvalidOperation(max_digits.unwrap_or(28)));
            }
        }

        if digits.len() > precision {
            return Err(PyDecimalError::InvalidOperation(max_digits.unwrap_or(28)));
        }

        Ok(Self(Kind::Finite(Finite {
            negative: finite.negative,
            digits,
            exp: target_exp,
        })))
    }

    fn fmt_python(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Kind::NaN {
                negative,
                signaling,
            } => {
                if *negative {
                    f.write_str("-")?;
                }
                f.write_str(if *signaling { "sNaN" } else { "NaN" })
            }
            Kind::Infinity { negative } => {
                if *negative {
                    f.write_str("-")?;
                }
                f.write_str("Infinity")
            }
            Kind::Finite(finite) => {
                if finite.negative {
                    f.write_str("-")?;
                }
                let digits = finite.digits.as_slice();
                let left_digits = i128::from(finite.exp) + digits.len() as i128;
                if finite.exp <= 0 && left_digits > -6 {
                    write_plain(f, digits, left_digits)
                } else {
                    write_scientific(f, digits, left_digits)
                }
            }
        }
    }

    fn fmt_fixed(&self, out: &mut impl fmt::Write) -> fmt::Result {
        match &self.0 {
            Kind::NaN {
                negative,
                signaling,
            } => {
                if *negative {
                    out.write_str("-")?;
                }
                out.write_str(if *signaling { "sNaN" } else { "NaN" })
            }
            Kind::Infinity { negative } => {
                if *negative {
                    out.write_str("-")?;
                }
                out.write_str("Infinity")
            }
            Kind::Finite(finite) => {
                if finite.negative {
                    out.write_str("-")?;
                }
                let digits = finite.digits.as_slice();
                if finite.exp >= 0 {
                    out.write_str(bytes_to_str(digits))?;
                    // CPython prints a zero coefficient without the exponent's
                    // zeros: format(Decimal("0E+3"), "f") == "0".
                    if digits != b"0" {
                        write_zeros(out, finite.exp as usize)?;
                    }
                } else {
                    let fraction_len = finite.exp.unsigned_abs() as usize;
                    if digits.len() > fraction_len {
                        let split = digits.len() - fraction_len;
                        out.write_str(bytes_to_str(&digits[..split]))?;
                        out.write_str(".")?;
                        out.write_str(bytes_to_str(&digits[split..]))?;
                    } else {
                        out.write_str("0.")?;
                        write_zeros(out, fraction_len - digits.len())?;
                        out.write_str(bytes_to_str(digits))?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// `format(value, "f")` as a `Display` adapter.
pub(crate) struct FixedDecimal<'a>(&'a PyDecimal);

impl fmt::Display for FixedDecimal<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_fixed(f)
    }
}

impl fmt::Display for PyDecimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_python(f)
    }
}

impl serde::Serialize for PyDecimal {
    /// `str(Decimal)`, i.e. kombu's decimal envelope value and Django's
    /// `DjangoJSONEncoder` representation.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl FromStr for PyDecimal {
    type Err = PyDecimalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PyDecimal::parse(s)
    }
}

fn write_plain(f: &mut fmt::Formatter<'_>, digits: &[u8], point: i128) -> fmt::Result {
    if point <= 0 {
        f.write_str("0.")?;
        write_zeros_fmt(f, point.unsigned_abs() as usize)?;
        f.write_str(bytes_to_str(digits))
    } else {
        let point = point as usize;
        if point < digits.len() {
            f.write_str(bytes_to_str(&digits[..point]))?;
            f.write_str(".")?;
            f.write_str(bytes_to_str(&digits[point..]))
        } else {
            f.write_str(bytes_to_str(digits))?;
            write_zeros_fmt(f, point - digits.len())
        }
    }
}

fn write_scientific(f: &mut fmt::Formatter<'_>, digits: &[u8], left_digits: i128) -> fmt::Result {
    f.write_str(bytes_to_str(&digits[..1]))?;
    if digits.len() > 1 {
        f.write_str(".")?;
        f.write_str(bytes_to_str(&digits[1..]))?;
    }
    f.write_str("E")?;
    let exponent = left_digits - 1;
    if exponent < 0 {
        f.write_str("-")?;
    } else {
        f.write_str("+")?;
    }
    write!(f, "{}", exponent.unsigned_abs())
}

fn write_zeros_fmt(f: &mut fmt::Formatter<'_>, count: usize) -> fmt::Result {
    for _ in 0..count {
        f.write_str("0")?;
    }
    Ok(())
}

fn write_zeros(out: &mut impl fmt::Write, count: usize) -> fmt::Result {
    for _ in 0..count {
        out.write_str("0")?;
    }
    Ok(())
}

fn bytes_to_str(digits: &[u8]) -> &str {
    // The digits are always ASCII produced by this module or validated by
    // `parse_digit_run`.
    std::str::from_utf8(digits).unwrap_or("0")
}

fn is_nan_payload(lower: &str, prefix: &str) -> bool {
    match lower.strip_prefix(prefix) {
        Some(rest) => !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()),
        None => false,
    }
}

fn parse_exponent(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits = parse_digit_run(digits)?;
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for digit in digits {
        value = value
            .checked_mul(10)?
            .checked_add(i64::from(digit - b'0'))?;
    }
    Some(if negative { -value } else { value })
}

/// Parse a run of decimal digits, allowing Python's single underscores
/// between digits. Returns `None` if the run is malformed.
fn parse_digit_run(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut digits = Vec::with_capacity(bytes.len());
    for (index, &byte) in bytes.iter().enumerate() {
        if byte.is_ascii_digit() {
            digits.push(byte);
        } else if byte == b'_' {
            let before = index > 0 && bytes[index - 1].is_ascii_digit();
            let after = bytes.get(index + 1).is_some_and(u8::is_ascii_digit);
            if !before || !after {
                return None;
            }
        } else {
            return None;
        }
    }
    Some(digits)
}

#[cfg(test)]
mod tests {
    use super::PyDecimal;

    fn py(text: &str) -> PyDecimal {
        PyDecimal::parse(text).expect("valid decimal")
    }

    #[test]
    fn parses_like_python_decimal() {
        assert_eq!(py("0001.5").to_string(), "1.5");
        assert_eq!(py("0.00").to_string(), "0.00");
        assert_eq!(py("000").to_string(), "0");
        assert_eq!(py("-0.0").to_string(), "-0.0");
        assert_eq!(py("1.2300").to_string(), "1.2300");
        assert_eq!(py("1e+0").to_string(), "1");
        assert_eq!(py("5.").to_string(), "5");
        assert_eq!(py("5.e3").to_string(), "5E+3");
        assert_eq!(py("1_000").to_string(), "1000");
        assert_eq!(py(" 1.5 ").to_string(), "1.5");
        assert_eq!(py("+1.5").to_string(), "1.5");
        assert_eq!(py(".5").to_string(), "0.5");
        assert_eq!(py("1e-0").to_string(), "1");
    }

    #[test]
    fn python_str_rules() {
        // Table produced by CPython 3.12.
        let cases = [
            ("1E+3", "1E+3"),
            ("1E+0", "1"),
            ("1E+1", "1E+1"),
            ("1.23E+5", "1.23E+5"),
            ("0E+3", "0E+3"),
            ("-0E+3", "-0E+3"),
            ("1E-6", "0.000001"),
            ("1E-7", "1E-7"),
            ("0.000001", "0.000001"),
            ("0.0000001", "1E-7"),
            ("12.3E-8", "1.23E-7"),
            ("100", "100"),
            ("100.0", "100.0"),
            ("1234500.0", "1234500.0"),
            ("0.00", "0.00"),
            ("123", "123"),
            ("0E-5", "0.00000"),
            ("1.0E+2", "1.0E+2"),
            ("1234E-2", "12.34"),
            ("1e-999999999", "1E-999999999"),
            ("1e999999999", "1E+999999999"),
        ];
        for (input, expected) in cases {
            assert_eq!(py(input).to_string(), expected, "input {input}");
        }
    }

    #[test]
    fn python_fixed_rules() {
        let cases = [
            ("1E+3", "1000"),
            ("1E+0", "1"),
            ("1.23E+5", "123000"),
            ("0E+3", "0"),
            ("-0E+3", "-0"),
            ("1E-6", "0.000001"),
            ("1E-7", "0.0000001"),
            ("0.00", "0.00"),
            ("0E-5", "0.00000"),
            ("1234E-2", "12.34"),
            ("-0.00", "-0.00"),
        ];
        for (input, expected) in cases {
            assert_eq!(py(input).to_fixed_string(), expected, "input {input}");
        }
    }

    #[test]
    fn quantize_half_even_and_precision() {
        // Numbers checked against DRF's DecimalField in CPython.
        let cases = [
            (12, 4, "1.1000", Some("1.1000")),
            (12, 4, "1.23455", Some("1.2346")),
            (12, 4, "1.23445", Some("1.2344")),
            (12, 2, "1.005", Some("1.00")),
            (12, 2, "2.675", Some("2.68")),
            (5, 2, "12345.678", None),
            (2, 2, "1.00", None),
            (3, 2, "0.005", Some("0.00")),
            (4, 3, "0.0005", Some("0.000")),
            (4, 3, "0.0015", Some("0.002")),
            (4, 3, "0.0025", Some("0.002")),
            (12, 4, "99999999.99995", None),
            (12, 4, "1E+10", None),
        ];
        for (max_digits, decimal_places, input, expected) in cases {
            let result = py(input).quantize(decimal_places, Some(max_digits));
            match (result, expected) {
                (Ok(value), Some(expected)) => {
                    assert_eq!(
                        value.to_fixed_string(),
                        expected,
                        "input {input} max_digits {max_digits} dp {decimal_places}"
                    );
                }
                (Err(_), None) => {}
                (result, expected) => panic!(
                    "input {input} max_digits {max_digits} dp {decimal_places}: got {result:?}, expected {expected:?}"
                ),
            }
        }
    }

    #[test]
    fn float_conversion() {
        assert_eq!(py("1.10").to_f64(), 1.1);
        assert_eq!(py("1E+3").to_f64(), 1000.0);
        assert_eq!(py("1E-6").to_f64(), 1e-6);
        assert_eq!(py("-0.00").to_f64().to_bits(), (-0.0f64).to_bits());
        assert!(py("1e999999999").to_f64().is_infinite());
        assert_eq!(py("1e-999999999").to_f64(), 0.0);
        assert!(py("NaN").to_f64().is_nan());
    }
}
