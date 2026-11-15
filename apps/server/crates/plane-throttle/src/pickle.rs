// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Just enough of CPython's pickle for DRF throttle histories.
//!
//! `django_redis.serializers.pickle.PickleSerializer` writes
//! `pickle.dumps(value, pickle.DEFAULT_PROTOCOL)`; on Python 3.12
//! `DEFAULT_PROTOCOL` is **4** (`django_redis/serializers/pickle.py:10-12`,
//! `HTTP_CONTRACT.md` §12.4). No `PICKLE_VERSION` is configured by Plane
//! (`plane/settings/common.py:274-291`), so every throttle value on the wire is
//! a protocol-4 pickle of a `list` of `float`s.
//!
//! The bytes must match exactly, not merely decode: both backends write the
//! same key, and the differential harness compares raw Redis values. The
//! encoder therefore reproduces CPython's opcode stream and its framing rules
//! (`Lib/pickle.py`, `_Framer` / `save_list` / `save_float`):
//!
//! * `PROTO 4`, then a framer that buffers opcodes and flushes a `FRAME`
//!   header when the buffer holds at least `_FRAME_SIZE_TARGET` (64 KiB) bytes
//!   at the *start of the next `save()`*, and once more at the end;
//! * a frame shorter than `_FRAME_SIZE_MIN` (4) bytes is written **without** a
//!   `FRAME` header — which is why `[]` serialises to the five bytes
//!   `80 04 5d 94 2e` and not to a frame;
//! * `EMPTY_LIST`, `MEMOIZE`, then `BINFLOAT` + 8 big-endian IEEE-754 bytes per
//!   value: `APPEND` for one value, `MARK` … `APPENDS` for two or more (never
//!   `APPEND` repeated);
//! * `STOP` last.
//!
//! Verified byte for byte against `pickle.dumps(..., 4)` in the tests below and
//! in `tools/throttle/interop.py`.

const PROTO: u8 = 0x80;
const FRAME: u8 = 0x95;
const STOP: u8 = b'.';
const EMPTY_LIST: u8 = b']';
const LIST: u8 = b'l';
const MARK: u8 = b'(';
const APPEND: u8 = b'a';
const APPENDS: u8 = b'e';
const MEMOIZE: u8 = 0x94;
const BINFLOAT: u8 = b'G';
/// `pickle._Framer._FRAME_SIZE_MIN`.
const FRAME_SIZE_MIN: usize = 4;
/// `pickle._Framer._FRAME_SIZE_TARGET`.
const FRAME_SIZE_TARGET: usize = 64 * 1024;
/// The protocol Plane's `PickleSerializer` writes (`pickle.DEFAULT_PROTOCOL`).
pub const DEFAULT_PROTOCOL: u8 = 4;

/// Why a value is not a pickle of a list of floats.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PickleError {
    #[error("truncated pickle: {0}")]
    Truncated(&'static str),
    #[error("unsupported pickle opcode 0x{0:02x}")]
    Unsupported(u8),
    #[error("pickle is not a list of floats")]
    NotAFloatList,
    #[error("invalid FLOAT text {0:?}")]
    InvalidFloat(String),
    #[error("trailing bytes after STOP")]
    TrailingBytes,
}

/// `pickle.dumps(values, 4)`: the exact bytes `PickleSerializer.dumps` writes.
pub fn dumps_floats(values: &[f64]) -> Vec<u8> {
    // PROTO is written before framing starts (`pickle._Pickler.dump`).
    let mut out = Vec::with_capacity(16 + values.len() * 9);
    out.push(PROTO);
    out.push(DEFAULT_PROTOCOL);

    let mut frame = Vec::with_capacity(3 + values.len() * 9);
    frame.push(EMPTY_LIST);
    frame.push(MEMOIZE);
    match values {
        [] => {}
        [value] => {
            frame.push(BINFLOAT);
            frame.extend_from_slice(&value.to_bits().to_be_bytes());
            frame.push(APPEND);
        }
        _ => {
            frame.push(MARK);
            for value in values {
                // `save()` calls `commit_frame()` before writing each item.
                if frame.len() >= FRAME_SIZE_TARGET {
                    commit_frame(&mut out, &mut frame);
                }
                frame.push(BINFLOAT);
                frame.extend_from_slice(&value.to_bits().to_be_bytes());
            }
            frame.push(APPENDS);
        }
    }
    frame.push(STOP);
    commit_frame(&mut out, &mut frame);
    out
}

/// Write one frame: the `FRAME` opcode is omitted for payloads shorter than
/// `pickle._Framer._FRAME_SIZE_MIN`.
fn commit_frame(out: &mut Vec<u8>, frame: &mut Vec<u8>) {
    if frame.is_empty() {
        return;
    }
    if frame.len() >= FRAME_SIZE_MIN {
        out.push(FRAME);
        out.extend_from_slice(&(frame.len() as u64).to_le_bytes());
    }
    out.append(frame);
}

/// `pickle.loads` restricted to a top-level `list` of floats.
///
/// Accepts every opcode CPython emits for that shape in protocols 0-5,
/// including `FLOAT` (protocol 0) and the memo bookkeeping opcodes, so a value
/// written by a deployment that ever set `PICKLE_VERSION` still decodes.
/// Anything else is an error, which is what Django would turn into a 500.
pub fn loads_floats(bytes: &[u8]) -> Result<Vec<f64>, PickleError> {
    let mut reader = Reader { bytes, position: 0 };
    let mut values: Vec<f64> = Vec::new();
    let mut pending: Vec<f64> = Vec::new();
    let mut in_mark = false;
    let mut saw_list = false;

    while let Some(opcode) = reader.byte()? {
        match opcode {
            PROTO => {
                reader.skip(1, "PROTO version")?;
            }
            FRAME => {
                reader.skip(8, "FRAME length")?;
            }
            EMPTY_LIST | LIST => {
                saw_list = true;
                values.clear();
            }
            MEMOIZE => {}
            // Memo bookkeeping carries no value for a list of floats.
            0x71 => reader.skip(1, "BINPUT")?,
            0x72 => reader.skip(4, "LONG_BINPUT")?,
            b'p' => reader.skip_line("PUT")?,
            BINFLOAT => {
                let bits = reader.take(8, "BINFLOAT")?;
                let mut array = [0_u8; 8];
                array.copy_from_slice(bits);
                push(
                    f64::from_bits(u64::from_be_bytes(array)),
                    in_mark,
                    &mut values,
                    &mut pending,
                );
            }
            b'F' => {
                let text = reader.line("FLOAT")?;
                let value: f64 = text
                    .parse()
                    .map_err(|_| PickleError::InvalidFloat(text.to_string()))?;
                push(value, in_mark, &mut values, &mut pending);
            }
            MARK => {
                in_mark = true;
                pending.clear();
            }
            // With `EMPTY_LIST` (protocol 1 and later) the value was already
            // pushed by BINFLOAT and APPEND only pops it back onto the list in
            // CPython, so there is nothing to do. With `MARK` + `LIST`
            // (protocol 0) the value is still pending.
            APPEND => {
                if in_mark && let Some(value) = pending.pop() {
                    values.push(value);
                }
            }
            APPENDS => {
                in_mark = false;
                values.append(&mut pending);
            }
            STOP => {
                if reader.remaining() > 0 {
                    return Err(PickleError::TrailingBytes);
                }
                return if saw_list {
                    Ok(values)
                } else {
                    Err(PickleError::NotAFloatList)
                };
            }
            other => return Err(PickleError::Unsupported(other)),
        }
    }
    Err(PickleError::Truncated("STOP"))
}

fn push(value: f64, in_mark: bool, values: &mut Vec<f64>, pending: &mut Vec<f64>) {
    if in_mark {
        pending.push(value);
    } else {
        values.push(value);
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn byte(&mut self) -> Result<Option<u8>, PickleError> {
        let Some(byte) = self.bytes.get(self.position) else {
            return Ok(None);
        };
        self.position += 1;
        Ok(Some(*byte))
    }

    fn take(&mut self, count: usize, what: &'static str) -> Result<&[u8], PickleError> {
        let end = self.position.saturating_add(count);
        let Some(slice) = self.bytes.get(self.position..end) else {
            return Err(PickleError::Truncated(what));
        };
        self.position = end;
        Ok(slice)
    }

    fn skip(&mut self, count: usize, what: &'static str) -> Result<(), PickleError> {
        self.take(count, what).map(|_| ())
    }

    fn skip_line(&mut self, what: &'static str) -> Result<(), PickleError> {
        self.line(what).map(|_| ())
    }

    fn line(&mut self, what: &'static str) -> Result<&str, PickleError> {
        let rest = &self.bytes[self.position..];
        let Some(end) = rest.iter().position(|byte| *byte == b'\n') else {
            return Err(PickleError::Truncated(what));
        };
        let line = &rest[..end];
        self.position += end + 1;
        std::str::from_utf8(line).map_err(|_| PickleError::Truncated(what))
    }
}

/// Lower-case hex, as `binascii.hexlify(...).decode()` renders raw Redis
/// values in the interop tool and the differential harness.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixtures produced by `apps/api/.venv/bin/python`:
    /// `binascii.hexlify(pickle.dumps(case, 4))`.
    #[test]
    fn matches_cpython_protocol_4_bytes() {
        let cases: &[(&[f64], &str)] = &[
            (&[], "80045d942e"),
            (
                &[1789000000.123456],
                "8004950d000000000000005d944741daa87e5007e6b4612e",
            ),
            (
                &[1789000000.123456, 1788999940.0],
                "80049517000000000000005d94284741daa87e5007e6b44741daa87e41000000652e",
            ),
            (
                &[1.0, 2.0, 3.0],
                "80049520000000000000005d9428473ff0000000000000474000000000000000474008000000000000652e",
            ),
            (&[0.0], "8004950d000000000000005d94470000000000000000612e"),
            (&[-0.0], "8004950d000000000000005d94478000000000000000612e"),
            (
                &[1789000000.0],
                "8004950d000000000000005d944741daa87e50000000612e",
            ),
        ];
        for (values, expected) in cases {
            let encoded = dumps_floats(values);
            assert_eq!(hex(&encoded), *expected, "encoding {values:?}");
            assert_eq!(loads_floats(&encoded).as_deref(), Ok(*values));
        }
    }

    #[test]
    fn extreme_floats_round_trip_byte_exactly() {
        let values = [1e-5, 1e16, 5e-324, f64::MAX, -1.5e-9, 100.0];
        // `pickle.dumps([1e-05, 1e+16, 5e-324, 1.7976931348623157e+308,
        // -1.5e-09, 100.0], 4)`
        let expected = "8004953b000000000000005d9428473ee4f8b588e368f1474341c37937e0800047000000000000000147\
7fefffffffffffff47be19c511dc3a41df474059000000000000652e";
        let encoded = dumps_floats(&values);
        assert_eq!(hex(&encoded), expected);
        assert_eq!(loads_floats(&encoded).as_deref(), Ok(&values[..]));

        // Persisted bits, including signed zero and subnormals.
        assert!(loads_floats(&dumps_floats(&[-0.0])).unwrap()[0].is_sign_negative());
    }

    #[test]
    fn empty_list_has_no_frame_header() {
        // `pickle._Framer._FRAME_SIZE_MIN` is 4 and the body of `[]` is three
        // bytes, so CPython writes it without a FRAME opcode.
        assert_eq!(dumps_floats(&[]), vec![0x80, 0x04, b']', 0x94, b'.']);
    }

    #[test]
    fn two_values_use_mark_and_appends() {
        let encoded = dumps_floats(&[1.0, 2.0]);
        // PROTO, FRAME, 8-byte length, then the first frame's payload.
        assert_eq!(&encoded[..3], &[0x80, 0x04, 0x95]);
        let payload = &encoded[11..];
        assert_eq!(payload[0], b']');
        assert_eq!(payload[1], 0x94);
        assert_eq!(payload[2], b'(');
        assert_eq!(payload[3], b'G');
        assert_eq!(payload[12], b'G');
        assert_eq!(payload[21], b'e');
        assert_eq!(payload[22], b'.');
        assert_eq!(payload.len(), 23);
        assert_eq!(encoded[3..11], (23_u64).to_le_bytes());
    }

    #[test]
    fn multiple_frames_for_a_very_long_history() {
        // A history this long cannot come from a sane rate limit, but the
        // framing rule must stay exact if one ever does: each frame is written
        // once its buffer reaches 64 KiB at an item boundary.
        let values: Vec<f64> = (0..20_000)
            .map(|index| 1_789_000_000.0 + index as f64)
            .collect();
        let encoded = dumps_floats(&values);

        let mut frames = 0;
        let mut position = 2;
        while position < encoded.len() && encoded[position] == FRAME {
            let length =
                u64::from_le_bytes(encoded[position + 1..position + 9].try_into().unwrap());
            position += 9 + length as usize;
            frames += 1;
        }
        // The force-flushed last frame ends with STOP.
        assert_eq!(encoded.last(), Some(&b'.'));
        assert_eq!(position, encoded.len());
        assert!(frames > 1, "expected several frames, saw {frames}");
        assert_eq!(loads_floats(&encoded).as_deref(), Ok(&values[..]));
    }

    #[test]
    fn protocol_0_float_text_and_protocol_2_memo() {
        // Python 3.12: `pickle.dumps([1.5], 0) == b'(lp0\nF1.5\na.'` and
        // `pickle.dumps([1.5], 2)` is `80 02 5d 71 00 47 <8 bytes> 61 2e`.
        assert_eq!(loads_floats(b"(lp0\nF1.5\na.").as_deref(), Ok(&[1.5][..]));
        assert_eq!(
            loads_floats(b"\x80\x02]q\x00G?\xf8\x00\x00\x00\x00\x00\x00a.").as_deref(),
            Ok(&[1.5][..])
        );
    }

    #[test]
    fn rejects_other_values() {
        assert_eq!(
            loads_floats(b"\x80\x04K\x05."),
            Err(PickleError::Unsupported(b'K'))
        );
        assert_eq!(loads_floats(b"\x80\x04."), Err(PickleError::NotAFloatList));
        assert_eq!(
            loads_floats(b"\x80\x04]\x94G?\xf8\x00\x00\x00\x00\x00\x00."),
            Ok(vec![1.5])
        );
        assert_eq!(loads_floats(b""), Err(PickleError::Truncated("STOP")));
        // A valid but truncated frame: no STOP.
        assert_eq!(
            loads_floats(b"\x80\x04\x95\x03\x00\x00\x00\x00\x00\x00\x00]\x94G"),
            Err(PickleError::Truncated("BINFLOAT"))
        );
        assert_eq!(loads_floats(b"\xff"), Err(PickleError::Unsupported(0xff)));
    }

    fn hex(bytes: &[u8]) -> String {
        to_hex(bytes)
    }
}
