// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! CPython's protocol-4 pickle, byte for byte, for the values Plane caches.
//!
//! `django_redis.serializers.pickle.PickleSerializer.dumps` is
//! `pickle.dumps(value, pickle.DEFAULT_PROTOCOL)`; on Python 3.12
//! `DEFAULT_PROTOCOL` is **4** (`django_redis/serializers/pickle.py:10-12`,
//! `HTTP_CONTRACT.md` §12.4). Plane configures no `PICKLE_VERSION`
//! (`plane/settings/common.py:274-291`), so every value on the wire is a
//! protocol-4 pickle.
//!
//! The encoder reproduces `Lib/pickle.py`'s opcode stream exactly, because
//! both backends write the same key and the differential harness compares raw
//! Redis values:
//!
//! * `PROTO 4`, then a framer that buffers opcodes and flushes a `FRAME` header
//!   when the buffer holds at least `_FRAME_SIZE_TARGET` (64 KiB) at the start
//!   of the next `save()`, and once more at the end;
//! * a frame shorter than `_FRAME_SIZE_MIN` (4) bytes is written **without** a
//!   `FRAME` header — which is why `[]` is the five bytes `80 04 5d 94 2e`;
//! * a `str` or `bytes` of 64 KiB or more is written outside the frame, after
//!   force-flushing it (`_Framer.write_large_bytes`);
//! * `MEMOIZE` after every memoised object and `BINGET` for a repeat reference,
//!   keyed on object identity (see [`crate::value`]);
//! * `_BATCHSIZE` (1000) batching for `APPENDS`/`SETITEMS`/`ADDITEMS`;
//! * the `__reduce__` protocols of `uuid.UUID`, `datetime.date`,
//!   `datetime.datetime`, `datetime.time`, `datetime.timedelta`,
//!   `datetime.timezone` and `decimal.Decimal`, including the interned module
//!   and class-name strings those global lookups re-use
//!   (`STACK_GLOBAL` + `MEMOIZE`).
//!
//! The decoder accepts everything the encoder writes plus protocols 0-5 for
//! the same value shapes, so a value written by a deployment that ever changed
//! `PICKLE_VERSION` still decodes. A pickle that would produce a recursive
//! object (a container memoised and referenced before it is complete) is
//! rejected: a [`PyValue`] is a tree, not a cyclic graph.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Datelike, NaiveDate, NaiveTime, TimeDelta, Timelike};
use uuid::Uuid;

use plane_serde::PyDecimal;
use plane_serde::pyjson::PyStr;

use crate::error::PickleError;
use crate::pyint::PyInt;
use crate::value::{PyDateTime, PyMapping, PyTz, PyValue, ReduceFunc};

/// `pickle.PROTO`.
const PROTO: u8 = 0x80;
/// `pickle.FRAME`.
const FRAME: u8 = 0x95;
/// `pickle.STOP`.
const STOP: u8 = b'.';
/// `pickle.MARK`.
const MARK: u8 = b'(';
/// `pickle.POP`.
const POP: u8 = b'0';
/// `pickle.POP_MARK`.
const POP_MARK: u8 = b'1';
/// `pickle.DUP`.
const DUP: u8 = b'2';
/// `pickle.NONE`.
const NONE: u8 = b'N';
/// `pickle.NEWTRUE`.
const NEWTRUE: u8 = 0x88;
/// `pickle.NEWFALSE`.
const NEWFALSE: u8 = 0x89;
/// `pickle.EMPTY_LIST`.
const EMPTY_LIST: u8 = b']';
/// `pickle.LIST`.
const LIST: u8 = b'l';
/// `pickle.APPEND`.
const APPEND: u8 = b'a';
/// `pickle.APPENDS`.
const APPENDS: u8 = b'e';
/// `pickle.EMPTY_TUPLE`.
const EMPTY_TUPLE: u8 = b')';
/// `pickle.TUPLE`.
const TUPLE: u8 = b't';
/// `pickle.TUPLE1`.
const TUPLE1: u8 = 0x85;
/// `pickle.TUPLE2`.
const TUPLE2: u8 = 0x86;
/// `pickle.TUPLE3`.
const TUPLE3: u8 = 0x87;
/// `pickle.EMPTY_DICT`.
const EMPTY_DICT: u8 = b'}';
/// `pickle.DICT`.
const DICT: u8 = b'd';
/// `pickle.SETITEM`.
const SETITEM: u8 = b's';
/// `pickle.SETITEMS`.
const SETITEMS: u8 = b'u';
/// `pickle.EMPTY_SET`.
const EMPTY_SET: u8 = 0x8f;
/// `pickle.ADDITEMS`.
const ADDITEMS: u8 = 0x90;
/// `pickle.FROZENSET`.
const FROZENSET: u8 = 0x91;
/// `pickle.MEMOIZE`.
const MEMOIZE: u8 = 0x94;
/// `pickle.BINPUT`.
const BINPUT: u8 = b'q';
/// `pickle.LONG_BINPUT`.
const LONG_BINPUT: u8 = b'r';
/// `pickle.PUT`.
const PUT: u8 = b'p';
/// `pickle.BINGET`.
const BINGET: u8 = b'h';
/// `pickle.LONG_BINGET`.
const LONG_BINGET: u8 = b'j';
/// `pickle.GET`.
const GET: u8 = b'g';
/// `pickle.STACK_GLOBAL`.
const STACK_GLOBAL: u8 = 0x93;
/// `pickle.GLOBAL`.
const GLOBAL: u8 = b'c';
/// `pickle.REDUCE`.
const REDUCE: u8 = b'R';
/// `pickle.NEWOBJ`.
const NEWOBJ: u8 = 0x81;
/// `pickle.BUILD`.
const BUILD: u8 = b'b';
/// `pickle.BININT`.
const BININT: u8 = b'J';
/// `pickle.BININT1`.
const BININT1: u8 = b'K';
/// `pickle.BININT2`.
const BININT2: u8 = b'M';
/// `pickle.LONG1`.
const LONG1: u8 = 0x8a;
/// `pickle.LONG4`.
const LONG4: u8 = 0x8b;
/// `pickle.LONG`.
const LONG: u8 = b'L';
/// `pickle.INT`.
const INT: u8 = b'I';
/// `pickle.BINFLOAT`.
const BINFLOAT: u8 = b'G';
/// `pickle.FLOAT`.
const FLOAT: u8 = b'F';
/// `pickle.SHORT_BINUNICODE`.
const SHORT_BINUNICODE: u8 = 0x8c;
/// `pickle.BINUNICODE`.
const BINUNICODE: u8 = 0x58;
/// `pickle.BINUNICODE8`.
const BINUNICODE8: u8 = 0x8d;
/// `pickle.UNICODE`.
const UNICODE: u8 = b'V';
/// `pickle.SHORT_BINBYTES`.
const SHORT_BINBYTES: u8 = 0x43;
/// `pickle.BINBYTES`.
const BINBYTES: u8 = b'B';
/// `pickle.BINBYTES8`.
const BINBYTES8: u8 = 0x8e;
/// `pickle.BINSTRING`.
const BINSTRING: u8 = b'T';
/// `pickle.SHORT_BINSTRING`.
const SHORT_BINSTRING: u8 = b'U';

/// `pickle._Framer._FRAME_SIZE_MIN`.
const FRAME_SIZE_MIN: usize = 4;
/// `pickle._Framer._FRAME_SIZE_TARGET`.
const FRAME_SIZE_TARGET: usize = 64 * 1024;
/// `pickle._Pickler._BATCHSIZE`.
const BATCH_SIZE: usize = 1000;
/// The protocol Plane's `PickleSerializer` writes (`pickle.DEFAULT_PROTOCOL`).
pub const DEFAULT_PROTOCOL: u8 = 4;

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// `pickle.dumps(value, 4)`: the exact bytes `PickleSerializer.dumps` writes.
#[must_use]
pub fn dumps(value: &PyValue) -> Vec<u8> {
    let mut encoder = Encoder::new();
    encoder.out.extend_from_slice(&[PROTO, DEFAULT_PROTOCOL]);
    encoder.save(value);
    encoder.write(&[STOP]);
    encoder.commit_frame(true);
    encoder.out
}

/// What a memo entry is keyed on.
///
/// `Value` is the address of the `Arc` inside a node, i.e. Python's `id()`
/// for the object that node stands for. The other variants are objects CPython
/// creates internally while reducing `uuid`/`datetime`/`decimal` values: the
/// interned module and class-name strings, the class objects themselves, and
/// the `"int"` state key `uuid.UUID.__getstate__` re-uses.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum MemoKey {
    Value(usize),
    Module(&'static str),
    ClassName(&'static str, &'static str),
    Class(&'static str, &'static str),
    Fixed(&'static str),
}

/// The `__reduce__` classes Plane's cached values can contain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Uuid,
    DateTime,
    Date,
    Time,
    TimeDelta,
    TimeZone,
    Decimal,
    BuiltinList,
    BuiltinDict,
}

impl Class {
    /// The `(module, name)` a `STACK_GLOBAL` resolves to.
    #[must_use]
    pub fn global(self) -> (&'static str, &'static str) {
        match self {
            Class::Uuid => ("uuid", "UUID"),
            Class::DateTime => ("datetime", "datetime"),
            Class::Date => ("datetime", "date"),
            Class::Time => ("datetime", "time"),
            Class::TimeDelta => ("datetime", "timedelta"),
            Class::TimeZone => ("datetime", "timezone"),
            Class::Decimal => ("decimal", "Decimal"),
            Class::BuiltinList => ("builtins", "list"),
            Class::BuiltinDict => ("builtins", "dict"),
        }
    }

    /// Resolve a `STACK_GLOBAL`/`GLOBAL` pair.
    #[must_use]
    pub fn from_global(module: &str, name: &str) -> Option<Self> {
        Some(match (module, name) {
            ("uuid", "UUID") => Class::Uuid,
            ("datetime", "datetime") => Class::DateTime,
            ("datetime", "date") => Class::Date,
            ("datetime", "time") => Class::Time,
            ("datetime", "timedelta") => Class::TimeDelta,
            ("datetime", "timezone") => Class::TimeZone,
            ("decimal", "Decimal") => Class::Decimal,
            ("builtins", "list") => Class::BuiltinList,
            ("builtins", "dict") => Class::BuiltinDict,
            _ => return None,
        })
    }
}

/// The pointer that identifies an `Arc`-shared node, as Python's `id()` would.
fn arc_id<T>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value) as *const () as usize
}

fn memo_key(value: &PyValue) -> Option<MemoKey> {
    Some(match value {
        PyValue::Str(value) => MemoKey::Value(arc_id(value)),
        PyValue::Bytes(value) => MemoKey::Value(arc_id(value)),
        PyValue::List(items)
        | PyValue::Tuple(items)
        | PyValue::Set(items)
        | PyValue::FrozenSet(items) => MemoKey::Value(arc_id(items)),
        PyValue::Dict(dict) => MemoKey::Value(arc_id(dict)),
        PyValue::Uuid(value) => MemoKey::Value(arc_id(value)),
        PyValue::Date(value) => MemoKey::Value(arc_id(value)),
        PyValue::DateTime(value) => MemoKey::Value(arc_id(value)),
        PyValue::Time(value) => MemoKey::Value(arc_id(value)),
        PyValue::TimeDelta(value) => MemoKey::Value(arc_id(value)),
        PyValue::TimeZone(value) => MemoKey::Value(arc_id(value)),
        PyValue::Decimal(value) => MemoKey::Value(arc_id(value)),
        PyValue::Reduce(reduce) => MemoKey::Value(arc_id(reduce)),
        // CPython never memoises these.
        PyValue::None | PyValue::Bool(_) | PyValue::Int(_) | PyValue::Float(_) => return None,
    })
}

struct Encoder {
    out: Vec<u8>,
    frame: Vec<u8>,
    memo: HashMap<MemoKey, u32>,
    next_index: u32,
}

impl Encoder {
    fn new() -> Self {
        Self {
            out: Vec::with_capacity(256),
            frame: Vec::with_capacity(FRAME_SIZE_TARGET),
            memo: HashMap::new(),
            next_index: 0,
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        self.frame.extend_from_slice(bytes);
    }

    /// `_Framer.commit_frame`: flush when the buffer is full, or unconditionally
    /// at the end of the stream. A frame shorter than four bytes gets no
    /// `FRAME` header.
    fn commit_frame(&mut self, force: bool) {
        if !force && self.frame.len() < FRAME_SIZE_TARGET {
            return;
        }
        if self.frame.is_empty() {
            return;
        }
        if self.frame.len() >= FRAME_SIZE_MIN {
            self.out.push(FRAME);
            self.out
                .extend_from_slice(&(self.frame.len() as u64).to_le_bytes());
        }
        self.out.append(&mut self.frame);
    }

    /// `_Framer.write_large_bytes`: end the current frame, then write the
    /// header and payload outside any frame.
    fn write_large(&mut self, header: &[u8], payload: &[u8]) {
        self.commit_frame(true);
        self.out.extend_from_slice(header);
        self.out.extend_from_slice(payload);
    }

    fn memoize(&mut self, key: MemoKey) {
        let index = self.next_index;
        self.next_index += 1;
        self.write(&[MEMOIZE]);
        self.memo.insert(key, index);
    }

    /// Allocate a memo index for a fresh object CPython memoises but which no
    /// later opcode can reference (a reduce argument tuple).
    fn memoize_fresh(&mut self) {
        self.next_index += 1;
        self.write(&[MEMOIZE]);
    }

    fn write_binget(&mut self, index: u32) {
        if index < 256 {
            self.write(&[BINGET, index as u8]);
        } else {
            self.write(&[LONG_BINGET]);
            self.write(&index.to_le_bytes());
        }
    }

    fn save(&mut self, value: &PyValue) {
        // `_Pickler.save` commits the frame before doing anything else.
        self.commit_frame(false);
        if let Some(key) = memo_key(value)
            && let Some(index) = self.memo.get(&key)
        {
            self.write_binget(*index);
            return;
        }
        match value {
            PyValue::None => self.write(&[NONE]),
            PyValue::Bool(true) => self.write(&[NEWTRUE]),
            PyValue::Bool(false) => self.write(&[NEWFALSE]),
            PyValue::Int(value) => self.save_long(value),
            PyValue::Float(value) => {
                self.write(&[BINFLOAT]);
                self.write(&value.to_bits().to_be_bytes());
            }
            PyValue::Str(value) => self.save_str(MemoKey::Value(arc_id(value)), value.as_wtf8()),
            PyValue::Bytes(value) => {
                self.save_bytes(MemoKey::Value(arc_id(value)), value);
            }
            PyValue::List(items) => {
                self.write(&[EMPTY_LIST]);
                self.memoize(MemoKey::Value(arc_id(items)));
                self.batch_appends(items);
            }
            PyValue::Tuple(items) => self.save_tuple(items),
            PyValue::Set(items) => {
                self.write(&[EMPTY_SET]);
                self.memoize(MemoKey::Value(arc_id(items)));
                if !items.is_empty() {
                    for batch in items.chunks(BATCH_SIZE) {
                        self.write(&[MARK]);
                        for item in batch {
                            self.save(item);
                        }
                        self.write(&[ADDITEMS]);
                    }
                    // Same quirk as `batch_setitems`: an exact multiple of
                    // 1000 ends with an empty `MARK ADDITEMS`.
                    if items.len() % BATCH_SIZE == 0 {
                        self.write(&[MARK]);
                        self.write(&[ADDITEMS]);
                    }
                }
            }
            PyValue::FrozenSet(items) => {
                self.write(&[MARK]);
                for item in items.iter() {
                    self.save(item);
                }
                self.write(&[FROZENSET]);
                self.memoize(MemoKey::Value(arc_id(items)));
            }
            PyValue::Dict(dict) => {
                self.write(&[EMPTY_DICT]);
                self.memoize(MemoKey::Value(arc_id(dict)));
                self.batch_setitems(&dict.entries);
            }
            PyValue::Uuid(value) => self.save_uuid(MemoKey::Value(arc_id(value)), value),
            PyValue::Date(value) => {
                let state = [
                    (value.year() >> 8) as u8,
                    (value.year() & 0xff) as u8,
                    value.month() as u8,
                    value.day() as u8,
                ];
                self.save_global(Class::Date);
                self.save_bytes_fresh(&state);
                self.write(&[TUPLE1]);
                self.memoize_fresh();
                self.write(&[REDUCE]);
                self.memoize(MemoKey::Value(arc_id(value)));
            }
            PyValue::Time(value) => {
                let micros = value.nanosecond() % 1_000_000_000 / 1_000;
                let state = [
                    value.hour() as u8,
                    value.minute() as u8,
                    value.second() as u8,
                    (micros >> 16) as u8,
                    (micros >> 8) as u8,
                    micros as u8,
                ];
                self.save_global(Class::Time);
                self.save_bytes_fresh(&state);
                self.write(&[TUPLE1]);
                self.memoize_fresh();
                self.write(&[REDUCE]);
                self.memoize(MemoKey::Value(arc_id(value)));
            }
            PyValue::DateTime(value) => self.save_datetime(MemoKey::Value(arc_id(value)), value),
            PyValue::TimeDelta(value) => {
                let args = timedelta_args(**value);
                self.save_reduce(Class::TimeDelta, &args, Some(MemoKey::Value(arc_id(value))));
            }
            PyValue::TimeZone(value) => self.save_timezone(MemoKey::Value(arc_id(value)), **value),
            PyValue::Decimal(value) => {
                let text = value.to_python_string();
                self.save_global(Class::Decimal);
                self.save_str_fresh(text.as_bytes());
                self.write(&[TUPLE1]);
                self.memoize_fresh();
                self.write(&[REDUCE]);
                self.memoize(MemoKey::Value(arc_id(value)));
            }
            PyValue::Reduce(reduce) => {
                let class = match reduce.func {
                    ReduceFunc::List => Class::BuiltinList,
                    ReduceFunc::Dict => Class::BuiltinDict,
                };
                self.commit_frame(false);
                self.save_global(class);
                self.commit_frame(false);
                self.save_args(std::slice::from_ref(&reduce.value));
                self.write(&[REDUCE]);
                self.memoize(MemoKey::Value(arc_id(reduce)));
            }
        }
    }

    fn save_long(&mut self, value: &PyInt) {
        if !value.is_negative()
            && let Some(small) = value.to_u64()
        {
            if small <= 0xff {
                self.write(&[BININT1, small as u8]);
                return;
            }
            if small <= 0xffff {
                self.write(&[BININT2]);
                self.write(&(small as u16).to_le_bytes());
                return;
            }
        }
        if let Some(small) = value.to_i64()
            && i32::try_from(small).is_ok()
        {
            self.write(&[BININT]);
            self.write(&(small as i32).to_le_bytes());
            return;
        }
        let encoded = value.to_twos_complement();
        if encoded.len() < 256 {
            self.write(&[LONG1, encoded.len() as u8]);
        } else {
            self.write(&[LONG4]);
            self.write(&(encoded.len() as i32).to_le_bytes());
        }
        self.write(&encoded);
    }

    /// `save_str`: `SHORT_BINUNICODE` up to 255 bytes, `BINUNICODE` up to
    /// 4 GiB (written outside the frame from 64 KiB), `BINUNICODE8` above.
    fn save_str(&mut self, key: MemoKey, wtf8: &[u8]) {
        self.commit_frame(false);
        if let Some(index) = self.memo.get(&key) {
            self.write_binget(*index);
            return;
        }
        self.write_str_opcode(wtf8);
        self.memoize(key);
    }

    /// A `str` object CPython creates fresh for each value (a reduce argument),
    /// so it is never shared: encode it and allocate a new memo index.
    fn save_str_fresh(&mut self, wtf8: &[u8]) {
        self.commit_frame(false);
        self.write_str_opcode(wtf8);
        self.memoize_fresh();
    }

    fn write_str_opcode(&mut self, wtf8: &[u8]) {
        let length = wtf8.len();
        if length <= 0xff {
            self.write(&[SHORT_BINUNICODE, length as u8]);
            self.write(wtf8);
        } else if length > u32::MAX as usize {
            let mut header = vec![BINUNICODE8];
            header.extend_from_slice(&(length as u64).to_le_bytes());
            self.write_large(&header, wtf8);
        } else if length >= FRAME_SIZE_TARGET {
            let mut header = vec![BINUNICODE];
            header.extend_from_slice(&(length as u32).to_le_bytes());
            self.write_large(&header, wtf8);
        } else {
            self.write(&[BINUNICODE]);
            self.write(&(length as u32).to_le_bytes());
            self.write(wtf8);
        }
    }

    /// `save_bytes`: `SHORT_BINBYTES`/`BINBYTES`/`BINBYTES8`.
    fn save_bytes(&mut self, key: MemoKey, bytes: &[u8]) {
        self.commit_frame(false);
        if let Some(index) = self.memo.get(&key) {
            self.write_binget(*index);
            return;
        }
        self.write_bytes_opcode(bytes);
        self.memoize(key);
    }

    /// A `bytes` object CPython creates fresh for each value (a `date`'s state,
    /// a datetime's state): never shared, always a new memo index.
    fn save_bytes_fresh(&mut self, bytes: &[u8]) {
        self.commit_frame(false);
        self.write_bytes_opcode(bytes);
        self.memoize_fresh();
    }

    fn write_bytes_opcode(&mut self, bytes: &[u8]) {
        let length = bytes.len();
        if length <= 0xff {
            self.write(&[SHORT_BINBYTES, length as u8]);
            self.write(bytes);
        } else if length > u32::MAX as usize {
            let mut header = vec![BINBYTES8];
            header.extend_from_slice(&(length as u64).to_le_bytes());
            self.write_large(&header, bytes);
        } else if length >= FRAME_SIZE_TARGET {
            let mut header = vec![BINBYTES];
            header.extend_from_slice(&(length as u32).to_le_bytes());
            self.write_large(&header, bytes);
        } else {
            self.write(&[BINBYTES]);
            self.write(&(length as u32).to_le_bytes());
            self.write(bytes);
        }
    }

    fn save_tuple(&mut self, items: &Arc<Vec<PyValue>>) {
        if items.is_empty() {
            // `save_tuple`'s empty branch returns without memoising, so two
            // empty tuples are two `EMPTY_TUPLE` opcodes.
            self.write(&[EMPTY_TUPLE]);
            return;
        }
        if items.len() <= 3 {
            for item in items.iter() {
                self.save(item);
            }
            self.write(&[match items.len() {
                1 => TUPLE1,
                2 => TUPLE2,
                _ => TUPLE3,
            }]);
            self.memoize(MemoKey::Value(arc_id(items)));
            return;
        }
        self.write(&[MARK]);
        for item in items.iter() {
            self.save(item);
        }
        self.write(&[TUPLE]);
        self.memoize(MemoKey::Value(arc_id(items)));
    }

    /// `save(args)` for a synthetic argument tuple (`save_reduce`'s `args`).
    fn save_args(&mut self, items: &[PyValue]) {
        if items.is_empty() {
            self.write(&[EMPTY_TUPLE]);
            return;
        }
        if items.len() <= 3 {
            for item in items {
                self.save(item);
            }
            self.write(&[match items.len() {
                1 => TUPLE1,
                2 => TUPLE2,
                _ => TUPLE3,
            }]);
        } else {
            self.write(&[MARK]);
            for item in items {
                self.save(item);
            }
            self.write(&[TUPLE]);
        }
        self.memoize_fresh();
    }

    /// `_pickle.c:batch_appends`, as measured against CPython 3.12:
    /// a one-item list is `APPEND`; anything longer is `MARK … APPENDS` in
    /// batches of 1000, and a *trailing* batch of one keeps the
    /// `MARK … APPENDS` form (unlike `Lib/pickle.py`, which would use
    /// `APPEND`). A length that is an exact multiple of 1000 gets no empty
    /// trailing batch.
    fn batch_appends(&mut self, items: &[PyValue]) {
        match items.len() {
            0 => {}
            1 => {
                self.save(&items[0]);
                self.write(&[APPEND]);
            }
            _ => {
                for batch in items.chunks(BATCH_SIZE) {
                    self.write(&[MARK]);
                    for item in batch {
                        self.save(item);
                    }
                    self.write(&[APPENDS]);
                }
            }
        }
    }

    /// `_pickle.c:batch_setitems`: a one-item dict is `SETITEM`; anything
    /// longer is `MARK … SETITEMS` in batches of 1000, **including an empty
    /// trailing batch** when the length is an exact multiple of 1000
    /// (`pickle.dumps({…: …}, 4)` for 1000 keys ends `MARK SETITEMS`).
    fn batch_setitems(&mut self, entries: &[(PyValue, PyValue)]) {
        match entries.len() {
            0 => {}
            1 => {
                self.save(&entries[0].0);
                self.save(&entries[0].1);
                self.write(&[SETITEM]);
            }
            length => {
                for batch in entries.chunks(BATCH_SIZE) {
                    self.write(&[MARK]);
                    for (key, value) in batch {
                        self.save(key);
                        self.save(value);
                    }
                    self.write(&[SETITEMS]);
                }
                if length % BATCH_SIZE == 0 {
                    self.write(&[MARK]);
                    self.write(&[SETITEMS]);
                }
            }
        }
    }

    fn save_global(&mut self, class: Class) {
        let (module, name) = class.global();
        // `save(cls)` checks the memo before dispatch, so the second reference
        // to a class is one `BINGET` — the module and class-name strings are
        // not re-saved (`Lib/pickle.py:_Pickler.save`).
        let class_key = MemoKey::Class(module, name);
        if let Some(index) = self.memo.get(&class_key) {
            self.write_binget(*index);
            return;
        }
        // `save_global` calls `save(module_name)` and `save(name)`; each of
        // those is a `save()` and therefore commits the frame.
        self.save_str(MemoKey::Module(module), module.as_bytes());
        self.save_str(MemoKey::ClassName(module, name), name.as_bytes());
        self.write(&[STACK_GLOBAL]);
        self.memoize(class_key);
    }

    /// `save_reduce` for the simple `(class, args)` case: global, args tuple,
    /// `REDUCE`, `MEMOIZE`.
    ///
    /// `key` is `None` for a reduced object CPython creates fresh for each
    /// value (a `timezone`'s offset `timedelta`), which is never shared.
    fn save_reduce(&mut self, class: Class, args: &[PyValue], key: Option<MemoKey>) {
        self.commit_frame(false);
        self.save_global(class);
        self.commit_frame(false);
        self.save_args(args);
        self.write(&[REDUCE]);
        match key {
            Some(key) => self.memoize(key),
            None => self.memoize_fresh(),
        }
    }

    fn save_uuid(&mut self, key: MemoKey, value: &Uuid) {
        // `uuid.UUID.__reduce_ex__(4)` is the `__newobj__` form:
        // `(copyreg.__newobj__, (UUID,), {"int": ...})`, which `save_reduce`
        // writes as cls, args, NEWOBJ, MEMOIZE, state, BUILD.
        self.commit_frame(false);
        self.save_global(Class::Uuid);
        self.commit_frame(false);
        self.save_args(&[]);
        self.write(&[NEWOBJ]);
        self.memoize(key);
        self.write(&[EMPTY_DICT]);
        self.memoize_fresh();
        self.save_str(MemoKey::Fixed("uuid-int"), b"int");
        self.save(&PyValue::Int(PyInt::from_u128(value.as_u128())));
        self.write(&[SETITEM]);
        self.write(&[BUILD]);
    }

    fn save_datetime(&mut self, key: MemoKey, value: &PyDateTime) {
        let micros = value.naive.and_utc().timestamp_subsec_micros();
        let month = value.naive.month() as u8 + if value.fold { 128 } else { 0 };
        let state = [
            (value.naive.year() >> 8) as u8,
            (value.naive.year() & 0xff) as u8,
            month,
            value.naive.day() as u8,
            value.naive.hour() as u8,
            value.naive.minute() as u8,
            value.naive.second() as u8,
            (micros >> 16) as u8,
            (micros >> 8) as u8,
            micros as u8,
        ];
        self.commit_frame(false);
        self.save_global(Class::DateTime);
        self.commit_frame(false);
        // The args tuple is `(state,)` or `(state, tzinfo)`; its elements are
        // saved in order, so the tzinfo's memo index is allocated before the
        // tuple's.
        if let Some(zone) = value.tz.as_deref() {
            self.save_bytes_fresh(&state);
            self.save_timezone(MemoKey::Value(arc_id_of_tz(value)), *zone);
            self.write(&[TUPLE2]);
            self.memoize_fresh();
        } else {
            self.save_bytes_fresh(&state);
            self.write(&[TUPLE1]);
            self.memoize_fresh();
        }
        self.write(&[REDUCE]);
        self.memoize(key);
    }

    fn save_timezone(&mut self, key: MemoKey, zone: PyTz) {
        self.commit_frame(false);
        self.save_global(Class::TimeZone);
        self.commit_frame(false);
        self.save_reduce(
            Class::TimeDelta,
            &timedelta_args(TimeDelta::seconds(i64::from(zone.offset_seconds()))),
            None,
        );
        self.write(&[TUPLE1]);
        self.memoize_fresh();
        self.write(&[REDUCE]);
        self.memoize(key);
    }
}

/// `timedelta(days, seconds, microseconds)` as the three integers
/// `datetime.timedelta.__reduce__` passes.
fn timedelta_args(value: TimeDelta) -> [PyValue; 3] {
    // Python's `timedelta.days` floors and `seconds` is always 0..86399,
    // whereas `chrono::TimeDelta::num_days` truncates toward zero.
    let total_seconds = i128::from(value.num_seconds());
    let days = total_seconds.div_euclid(86_400);
    let seconds = total_seconds.rem_euclid(86_400);
    let micros = i128::from(value.subsec_nanos() / 1_000);
    [
        PyValue::Int(PyInt::from_i64(days as i64)),
        PyValue::Int(PyInt::from_i64(seconds as i64)),
        PyValue::Int(PyInt::from_i64(micros as i64)),
    ]
}

/// The identity of the timezone `Arc` nested in a [`PyDateTime`].
fn arc_id_of_tz(value: &PyDateTime) -> usize {
    match value.tz.as_deref() {
        Some(tz) => tz as *const PyTz as *const () as usize,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// One item on the unpickler's stack.
#[derive(Clone, Debug)]
enum Item {
    Value(PyValue),
    Class(Class),
    /// A container under construction (a memoised `EMPTY_LIST`/`EMPTY_DICT`/
    /// `EMPTY_SET` that will be filled by `APPEND`/`SETITEM`/`ADDITEMS`).
    Building(usize),
    Mark,
}

/// A container being filled in place.
#[derive(Debug)]
enum Building {
    List(Vec<PyValue>),
    Dict(Vec<(PyValue, PyValue)>),
    Set(Vec<PyValue>),
}

/// `pickle.loads` restricted to the values Plane caches.
pub fn loads(bytes: &[u8]) -> Result<PyValue, PickleError> {
    Decoder::new(bytes).run()
}

struct Decoder<'a> {
    reader: Reader<'a>,
    stack: Vec<Item>,
    memo: Vec<Item>,
    buildings: Vec<Option<Building>>,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            reader: Reader { bytes, position: 0 },
            stack: Vec::new(),
            memo: Vec::new(),
            buildings: Vec::new(),
        }
    }

    fn run(mut self) -> Result<PyValue, PickleError> {
        while let Some(opcode) = self.reader.byte()? {
            match opcode {
                PROTO => {
                    let protocol = self.reader.take(1, "PROTO version")?[0];
                    if protocol > 5 {
                        return Err(PickleError::UnsupportedProtocol(protocol));
                    }
                }
                FRAME => {
                    // `FRAME <n>` announces that the next `n` bytes are one
                    // frame; the opcodes inside it are parsed normally, so
                    // only the length is read here.
                    self.reader.skip(8, "FRAME length")?;
                }
                STOP => {
                    // Everything the pickle built is above the stack's base.
                    let value = self.pop_value("STOP")?;
                    if self.reader.remaining() > 0 {
                        return Err(PickleError::TrailingBytes);
                    }
                    if !self.stack.is_empty() {
                        return Err(PickleError::Malformed("items left on the stack"));
                    }
                    return Ok(value);
                }
                MARK => self.stack.push(Item::Mark),
                POP => {
                    self.pop()?;
                }
                POP_MARK => {
                    self.pop_to_mark()?;
                }
                DUP => {
                    let item = self
                        .stack
                        .last()
                        .cloned()
                        .ok_or(PickleError::Malformed("DUP of an empty stack"))?;
                    self.stack.push(item);
                }
                NONE => self.stack.push(Item::Value(PyValue::None)),
                NEWTRUE => self.stack.push(Item::Value(PyValue::Bool(true))),
                NEWFALSE => self.stack.push(Item::Value(PyValue::Bool(false))),
                BININT => {
                    let bytes = self.reader.take(4, "BININT")?;
                    let mut array = [0_u8; 4];
                    array.copy_from_slice(bytes);
                    let value = i32::from_le_bytes(array);
                    self.push_int(PyInt::from_i64(i64::from(value)));
                }
                BININT1 => {
                    let value = self.reader.take(1, "BININT1")?[0];
                    self.push_int(PyInt::from_i64(i64::from(value)));
                }
                BININT2 => {
                    let bytes = self.reader.take(2, "BININT2")?;
                    let mut array = [0_u8; 2];
                    array.copy_from_slice(bytes);
                    self.push_int(PyInt::from_i64(i64::from(u16::from_le_bytes(array))));
                }
                LONG1 => {
                    let length = self.reader.take(1, "LONG1 length")?[0] as usize;
                    let bytes = self.reader.take(length, "LONG1 body")?.to_vec();
                    self.push_int(PyInt::from_twos_complement(&bytes));
                }
                LONG4 => {
                    let bytes = self.reader.take(4, "LONG4 length")?;
                    let mut array = [0_u8; 4];
                    array.copy_from_slice(bytes);
                    let length = i32::from_le_bytes(array);
                    if length < 0 {
                        return Err(PickleError::Malformed("negative LONG4 length"));
                    }
                    let body = self.reader.take(length as usize, "LONG4 body")?.to_vec();
                    self.push_int(PyInt::from_twos_complement(&body));
                }
                INT => {
                    let text = self.reader.line("INT")?;
                    let int = match text {
                        "01" => PyInt::from_i64(1),
                        "00" => PyInt::from_i64(0),
                        other => PyInt::parse(other)
                            .ok_or_else(|| PickleError::InvalidInteger(other.to_string()))?,
                    };
                    self.push_int(int);
                }
                LONG => {
                    let text = self.reader.line("LONG")?;
                    let text = text.strip_suffix('L').unwrap_or(text);
                    let int = PyInt::parse(text)
                        .ok_or_else(|| PickleError::InvalidInteger(text.to_string()))?;
                    self.push_int(int);
                }
                BINFLOAT => {
                    let bytes = self.reader.take(8, "BINFLOAT")?;
                    let mut array = [0_u8; 8];
                    array.copy_from_slice(bytes);
                    self.stack.push(Item::Value(PyValue::Float(f64::from_bits(
                        u64::from_be_bytes(array),
                    ))));
                }
                FLOAT => {
                    let text = self.reader.line("FLOAT")?;
                    let value: f64 = text
                        .parse()
                        .map_err(|_| PickleError::InvalidFloat(text.to_string()))?;
                    self.stack.push(Item::Value(PyValue::Float(value)));
                }
                SHORT_BINUNICODE => {
                    let length = self.reader.take(1, "SHORT_BINUNICODE length")?[0] as usize;
                    let text = self.reader.take(length, "SHORT_BINUNICODE body")?.to_vec();
                    self.push_str(&text)?;
                }
                BINUNICODE => {
                    let length = self.reader.u32("BINUNICODE length")?;
                    let text = self.reader.take(length, "BINUNICODE body")?.to_vec();
                    self.push_str(&text)?;
                }
                BINUNICODE8 => {
                    let length = self.reader.u64("BINUNICODE8 length")?;
                    let text = self.reader.take(length, "BINUNICODE8 body")?.to_vec();
                    self.push_str(&text)?;
                }
                UNICODE => {
                    let line = self.reader.line_bytes("UNICODE")?;
                    let text = raw_unicode_escape(line);
                    self.push_str(&text)?;
                }
                SHORT_BINBYTES => {
                    let length = self.reader.take(1, "SHORT_BINBYTES length")?[0] as usize;
                    let bytes = self.reader.take(length, "SHORT_BINBYTES body")?.to_vec();
                    self.push_bytes(&bytes);
                }
                BINBYTES => {
                    let length = self.reader.u32("BINBYTES length")?;
                    let bytes = self.reader.take(length, "BINBYTES body")?.to_vec();
                    self.push_bytes(&bytes);
                }
                BINBYTES8 => {
                    let length = self.reader.u64("BINBYTES8 length")?;
                    let bytes = self.reader.take(length, "BINBYTES8 body")?.to_vec();
                    self.push_bytes(&bytes);
                }
                BINSTRING | SHORT_BINSTRING => {
                    let length = if opcode == BINSTRING {
                        self.reader.u32("BINSTRING length")?
                    } else {
                        self.reader.take(1, "SHORT_BINSTRING length")?[0] as usize
                    };
                    let bytes = self.reader.take(length, "BINSTRING body")?.to_vec();
                    // Protocol 0-2 `str`; a Python 3 `bytes` value round-trips
                    // through latin-1, which is what `pickle` writes.
                    self.push_bytes(&bytes);
                }
                EMPTY_LIST => {
                    let id = self.new_building(Building::List(Vec::new()));
                    self.stack.push(Item::Building(id));
                }
                LIST => {
                    let items = self.pop_mark_values()?;
                    self.stack.push(Item::Value(PyValue::List(Arc::new(items))));
                }
                APPEND => {
                    let value = self.pop_value("APPEND")?;
                    let id = self.building_id()?;
                    match self.building_mut(id)? {
                        Building::List(items) => items.push(value),
                        _ => return Err(PickleError::Malformed("APPEND to a non-list")),
                    }
                }
                APPENDS => {
                    let items = self.pop_mark_values()?;
                    let id = self.building_id()?;
                    match self.building_mut(id)? {
                        Building::List(target) => target.extend(items),
                        _ => return Err(PickleError::Malformed("APPENDS to a non-list")),
                    }
                }
                EMPTY_TUPLE => self.stack.push(Item::Value(PyValue::tuple(Vec::new()))),
                TUPLE => {
                    let items = self.pop_mark_values()?;
                    self.stack.push(Item::Value(PyValue::tuple(items)));
                }
                TUPLE1 | TUPLE2 | TUPLE3 => {
                    let count = usize::from(opcode - TUPLE1) + 1;
                    let items = self.pop_n_values(count, "TUPLE1/2/3")?;
                    self.stack.push(Item::Value(PyValue::tuple(items)));
                }
                EMPTY_DICT => {
                    let id = self.new_building(Building::Dict(Vec::new()));
                    self.stack.push(Item::Building(id));
                }
                DICT => {
                    let items = self.pop_mark_values()?;
                    self.stack.push(Item::Value(dict_from_pairs(items)?));
                }
                SETITEM => {
                    let value = self.pop_value("SETITEM")?;
                    let key = self.pop_value("SETITEM")?;
                    let id = self.building_id()?;
                    match self.building_mut(id)? {
                        Building::Dict(entries) => entries.push((key, value)),
                        _ => return Err(PickleError::Malformed("SETITEM on a non-dict")),
                    }
                }
                SETITEMS => {
                    let items = self.pop_mark_values()?;
                    let id = self.building_id()?;
                    let pairs = pairs_from_values(items)?;
                    match self.building_mut(id)? {
                        Building::Dict(entries) => entries.extend(pairs),
                        _ => return Err(PickleError::Malformed("SETITEMS on a non-dict")),
                    }
                }
                EMPTY_SET => {
                    let id = self.new_building(Building::Set(Vec::new()));
                    self.stack.push(Item::Building(id));
                }
                ADDITEMS => {
                    let items = self.pop_mark_values()?;
                    let id = self.building_id()?;
                    match self.building_mut(id)? {
                        Building::Set(target) => target.extend(items),
                        _ => return Err(PickleError::Malformed("ADDITEMS on a non-set")),
                    }
                }
                FROZENSET => {
                    let items = self.pop_mark_values()?;
                    self.stack
                        .push(Item::Value(PyValue::FrozenSet(Arc::new(items))));
                }
                MEMOIZE => {
                    let index = self.memo.len();
                    self.memoize(index)?;
                }
                BINPUT => {
                    let index = self.reader.take(1, "BINPUT")?[0] as usize;
                    self.memoize(index)?;
                }
                LONG_BINPUT => {
                    let index = self.reader.u32("LONG_BINPUT")?;
                    self.memoize(index)?;
                }
                PUT => {
                    let text = self.reader.line("PUT")?;
                    let index: usize = text
                        .parse()
                        .map_err(|_| PickleError::Malformed("invalid PUT index"))?;
                    self.memoize(index)?;
                }
                BINGET => {
                    let index = self.reader.take(1, "BINGET")?[0] as usize;
                    self.push_memo(index)?;
                }
                LONG_BINGET => {
                    let index = self.reader.u32("LONG_BINGET")?;
                    self.push_memo(index)?;
                }
                GET => {
                    let text = self.reader.line("GET")?;
                    let index: usize = text
                        .parse()
                        .map_err(|_| PickleError::Malformed("invalid GET index"))?;
                    self.push_memo(index)?;
                }
                STACK_GLOBAL => {
                    let name = self.pop()?;
                    let module = self.pop()?;
                    let (Item::Value(module), Item::Value(name)) = (module, name) else {
                        return Err(PickleError::Malformed("STACK_GLOBAL arguments"));
                    };
                    let (Some(module), Some(name)) = (module.as_str(), name.as_str()) else {
                        return Err(PickleError::Malformed("STACK_GLOBAL arguments"));
                    };
                    self.push_class(module, name)?;
                }
                GLOBAL => {
                    let module = self.reader.line("GLOBAL module")?.to_string();
                    let name = self.reader.line("GLOBAL name")?.to_string();
                    self.push_class(&module, &name)?;
                }
                NEWOBJ => {
                    let args = self.pop_value("NEWOBJ")?;
                    let class = self.pop()?;
                    let Item::Class(class) = class else {
                        return Err(PickleError::Malformed("NEWOBJ without a class"));
                    };
                    match class {
                        // `cls.__new__(cls, *args)` for `uuid.UUID` builds an
                        // empty instance whose state `BUILD` then fills in.
                        Class::Uuid => self.stack.push(Item::Value(PyValue::uuid(Uuid::nil()))),
                        other => {
                            let (module, name) = other.global();
                            return Err(PickleError::UnsupportedGlobal {
                                module: module.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }
                    // `args` is consumed; nothing to do with it.
                    let _ = args;
                }
                REDUCE => {
                    let args = self.pop_value("REDUCE")?;
                    let class = self.pop()?;
                    let Item::Class(class) = class else {
                        return Err(PickleError::Malformed("REDUCE without a class"));
                    };
                    let value = self.reduce(class, args)?;
                    self.stack.push(Item::Value(value));
                }
                BUILD => {
                    let state = self.pop_value("BUILD")?;
                    self.build(state)?;
                }
                other => return Err(PickleError::UnsupportedOpcode(other)),
            }
        }
        Err(PickleError::Truncated("STOP"))
    }

    fn push_int(&mut self, value: PyInt) {
        self.stack.push(Item::Value(PyValue::Int(value)));
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        self.stack.push(Item::Value(PyValue::bytes(bytes.to_vec())));
    }

    fn push_str(&mut self, bytes: &[u8]) -> Result<(), PickleError> {
        let text = PyStr::from_wtf8(bytes.to_vec()).map_err(|_| PickleError::InvalidString)?;
        self.stack.push(Item::Value(PyValue::Str(Arc::new(text))));
        Ok(())
    }

    fn push_class(&mut self, module: &str, name: &str) -> Result<(), PickleError> {
        match Class::from_global(module, name) {
            Some(class) => {
                self.stack.push(Item::Class(class));
                Ok(())
            }
            None => Err(PickleError::UnsupportedGlobal {
                module: module.to_string(),
                name: name.to_string(),
            }),
        }
    }

    fn push_memo(&mut self, index: usize) -> Result<(), PickleError> {
        let item = self
            .memo
            .get(index)
            .cloned()
            .ok_or(PickleError::InvalidMemoIndex(index))?;
        if let Item::Building(id) = item {
            // A reference to a container the pickler is still filling. In
            // protocol 4 a container is filled once, immediately after its
            // `EMPTY_*` opcode, so by the time anything references it the
            // contents are complete; take a snapshot. A *recursive* pickle
            // (the container references itself) cannot be represented and
            // shows up as a snapshot that is missing itself.
            let snapshot = self.snapshot_building(id)?;
            self.stack.push(Item::Value(snapshot.clone()));
            for entry in &mut self.memo {
                if matches!(entry, Item::Building(existing) if *existing == id) {
                    *entry = Item::Value(snapshot.clone());
                }
            }
            return Ok(());
        }
        self.stack.push(item);
        Ok(())
    }

    /// Clone the current contents of a building container into a [`PyValue`].
    fn snapshot_building(&self, id: usize) -> Result<PyValue, PickleError> {
        match self.buildings.get(id).and_then(Option::as_ref) {
            Some(Building::List(items)) => Ok(PyValue::List(Arc::new(items.clone()))),
            Some(Building::Dict(entries)) => Ok(PyValue::Dict(Arc::new(PyMapping {
                entries: entries.clone(),
            }))),
            Some(Building::Set(items)) => Ok(PyValue::Set(Arc::new(items.clone()))),
            None => Err(PickleError::Malformed("container already finished")),
        }
    }

    fn memoize(&mut self, index: usize) -> Result<(), PickleError> {
        let item = self
            .stack
            .last()
            .cloned()
            .ok_or(PickleError::Malformed("memoize of an empty stack"))?;
        if self.memo.len() <= index {
            self.memo.resize(index + 1, Item::Mark);
        }
        self.memo[index] = item;
        Ok(())
    }

    fn new_building(&mut self, building: Building) -> usize {
        self.buildings.push(Some(building));
        self.buildings.len() - 1
    }

    fn building_mut(&mut self, id: usize) -> Result<&mut Building, PickleError> {
        self.buildings
            .get_mut(id)
            .and_then(Option::as_mut)
            .ok_or(PickleError::Malformed("container already finished"))
    }

    fn building_id(&self) -> Result<usize, PickleError> {
        match self.stack.last() {
            Some(Item::Building(id)) => Ok(*id),
            _ => Err(PickleError::Malformed("append/setitem without a container")),
        }
    }

    /// Finish the container at the top of the stack: build it, then repoint
    /// every memo entry that referenced it.
    fn finish_building(&mut self, id: usize) -> Result<PyValue, PickleError> {
        let building = self
            .buildings
            .get_mut(id)
            .and_then(Option::take)
            .ok_or(PickleError::Malformed("container finished twice"))?;
        let value = match building {
            Building::List(items) => PyValue::List(Arc::new(items)),
            Building::Dict(entries) => PyValue::Dict(Arc::new(PyMapping { entries })),
            Building::Set(items) => PyValue::Set(Arc::new(items)),
        };
        for entry in &mut self.memo {
            if matches!(entry, Item::Building(existing) if *existing == id) {
                *entry = Item::Value(value.clone());
            }
        }
        Ok(value)
    }

    fn reduce(&mut self, class: Class, args: PyValue) -> Result<PyValue, PickleError> {
        let PyValue::Tuple(items) = args else {
            return Err(PickleError::Malformed("REDUCE arguments are not a tuple"));
        };
        match class {
            Class::Date => {
                let state = bytes_at(&items, 0)?;
                if state.len() != 4 {
                    return Err(PickleError::Malformed("date state is not 4 bytes"));
                }
                let year = u16::from(state[0]) << 8 | u16::from(state[1]);
                let date = NaiveDate::from_ymd_opt(
                    i32::from(year),
                    u32::from(state[2]),
                    u32::from(state[3]),
                )
                .ok_or(PickleError::Malformed("invalid date"))?;
                Ok(PyValue::date(date))
            }
            Class::Time => {
                let state = bytes_at(&items, 0)?;
                if state.len() != 6 {
                    return Err(PickleError::Malformed("time state is not 6 bytes"));
                }
                let hour = u32::from(state[0] & 0x7f);
                let micros =
                    u32::from(state[3]) << 16 | u32::from(state[4]) << 8 | u32::from(state[5]);
                let time = NaiveTime::from_hms_micro_opt(
                    hour,
                    u32::from(state[1]),
                    u32::from(state[2]),
                    micros,
                )
                .ok_or(PickleError::Malformed("invalid time"))?;
                Ok(PyValue::Time(Arc::new(time)))
            }
            Class::DateTime => {
                let state = bytes_at(&items, 0)?;
                if state.len() != 10 {
                    return Err(PickleError::Malformed("datetime state is not 10 bytes"));
                }
                let year = u16::from(state[0]) << 8 | u16::from(state[1]);
                let month = u32::from(state[2] & 0x7f);
                let fold = state[2] & 0x80 != 0;
                let micros =
                    u32::from(state[7]) << 16 | u32::from(state[8]) << 8 | u32::from(state[9]);
                let naive = NaiveDate::from_ymd_opt(i32::from(year), month, u32::from(state[3]))
                    .and_then(|date| {
                        date.and_hms_micro_opt(
                            u32::from(state[4]),
                            u32::from(state[5]),
                            u32::from(state[6]),
                            micros,
                        )
                    })
                    .ok_or(PickleError::Malformed("invalid datetime"))?;
                let tz = match items.get(1) {
                    Some(PyValue::TimeZone(zone)) => Some(Arc::new(**zone)),
                    Some(PyValue::None) | None => None,
                    Some(_) => {
                        return Err(PickleError::Malformed("datetime tzinfo is not a timezone"));
                    }
                };
                Ok(PyValue::DateTime(Arc::new(PyDateTime { naive, tz, fold })))
            }
            Class::TimeDelta => {
                let days = int_at(&items, 0)?;
                let seconds = int_at(&items, 1)?;
                let micros = int_at(&items, 2)?;
                let value = TimeDelta::days(days)
                    + TimeDelta::seconds(seconds)
                    + TimeDelta::microseconds(micros);
                Ok(PyValue::TimeDelta(Arc::new(value)))
            }
            Class::TimeZone => {
                let PyValue::TimeDelta(delta) = &items[0] else {
                    return Err(PickleError::Malformed("timezone without a timedelta"));
                };
                let seconds = delta.num_seconds();
                let offset = i32::try_from(seconds)
                    .map_err(|_| PickleError::Malformed("timezone offset out of range"))?;
                if offset % 60 != 0 || offset.abs() >= 86_400 {
                    return Err(PickleError::Malformed("invalid timezone offset"));
                }
                let zone = if offset == 0 {
                    PyTz::Utc
                } else {
                    PyTz::Offset(offset)
                };
                Ok(PyValue::TimeZone(Arc::new(zone)))
            }
            Class::Decimal => {
                let Some(PyValue::Str(text)) = items.first() else {
                    return Err(PickleError::Malformed("Decimal without a string"));
                };
                let text = text
                    .as_str()
                    .ok_or(PickleError::Malformed("Decimal text is not UTF-8"))?;
                let decimal = PyDecimal::parse(text)
                    .map_err(|_| PickleError::InvalidDecimal(text.to_string()))?;
                Ok(PyValue::Decimal(Arc::new(decimal)))
            }
            Class::BuiltinList => match items.first() {
                Some(PyValue::List(items)) => Ok(PyValue::List(items.clone())),
                Some(PyValue::Tuple(items)) => Ok(PyValue::list(items.as_ref().clone())),
                _ => Err(PickleError::Malformed("list() of a non-sequence")),
            },
            Class::BuiltinDict => match items.first() {
                Some(PyValue::Dict(dict)) => Ok(PyValue::Dict(dict.clone())),
                _ => Err(PickleError::Malformed("dict() of a non-mapping")),
            },
            Class::Uuid => Err(PickleError::Malformed("uuid.UUID via REDUCE")),
        }
    }

    /// `load_build`: apply `state` to the object below it.
    fn build(&mut self, state: PyValue) -> Result<(), PickleError> {
        if state == PyValue::None {
            return Ok(());
        }
        let Some(Item::Value(current)) = self.stack.last_mut() else {
            return Err(PickleError::Malformed("BUILD without an instance"));
        };
        match (current, state) {
            (PyValue::Uuid(slot), PyValue::Dict(dict)) => {
                let Some(int) = dict.get("int") else {
                    return Err(PickleError::Malformed("UUID state without an int"));
                };
                let PyValue::Int(int) = int else {
                    return Err(PickleError::Malformed("UUID state int is not an int"));
                };
                let Some(bytes) = int.to_be_bytes_16() else {
                    return Err(PickleError::Malformed("UUID int out of range"));
                };
                let placeholder = Arc::clone(slot);
                *slot = Arc::new(Uuid::from_bytes(bytes));
                // `save_reduce` memoises the object *before* `BUILD` applies
                // its state, so the memo holds the empty instance. Repoint
                // every alias of it.
                for entry in &mut self.memo {
                    if let Item::Value(PyValue::Uuid(other)) = entry
                        && Arc::ptr_eq(other, &placeholder)
                    {
                        *entry = Item::Value(PyValue::Uuid(Arc::clone(slot)));
                    }
                }
                Ok(())
            }
            (PyValue::Uuid(_), _) => Err(PickleError::Malformed("UUID state is not a dict")),
            _ => Err(PickleError::UnsupportedBuild),
        }
    }

    fn pop(&mut self) -> Result<Item, PickleError> {
        self.stack
            .pop()
            .ok_or(PickleError::Malformed("pop from an empty stack"))
    }

    fn pop_value(&mut self, what: &'static str) -> Result<PyValue, PickleError> {
        match self.pop()? {
            Item::Value(value) => Ok(value),
            Item::Building(id) => self.finish_building(id),
            _ => Err(PickleError::Malformed(what)),
        }
    }

    fn pop_n_values(
        &mut self,
        count: usize,
        what: &'static str,
    ) -> Result<Vec<PyValue>, PickleError> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.pop_value(what)?);
        }
        out.reverse();
        Ok(out)
    }

    /// Pop everything above the topmost mark and drop the mark.
    fn pop_to_mark(&mut self) -> Result<Vec<Item>, PickleError> {
        let Some(position) = self
            .stack
            .iter()
            .rposition(|item| matches!(item, Item::Mark))
        else {
            return Err(PickleError::Malformed("no MARK"));
        };
        let items = self.stack.split_off(position + 1);
        self.stack.pop(); // the mark
        Ok(items)
    }

    fn pop_mark_values(&mut self) -> Result<Vec<PyValue>, PickleError> {
        let items = self.pop_to_mark()?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            match item {
                Item::Value(value) => out.push(value),
                Item::Building(id) => out.push(self.finish_building(id)?),
                Item::Class(_) | Item::Mark => {
                    return Err(PickleError::Malformed("class or mark in a value list"));
                }
            }
        }
        Ok(out)
    }
}

fn bytes_at(items: &[PyValue], index: usize) -> Result<&[u8], PickleError> {
    match items.get(index) {
        Some(PyValue::Bytes(bytes)) => Ok(bytes),
        _ => Err(PickleError::Malformed("expected a bytes argument")),
    }
}

fn int_at(items: &[PyValue], index: usize) -> Result<i64, PickleError> {
    match items.get(index) {
        Some(PyValue::Int(value)) => value
            .to_i64()
            .ok_or(PickleError::Malformed("integer out of range")),
        _ => Err(PickleError::Malformed("expected an int argument")),
    }
}

fn dict_from_pairs(items: Vec<PyValue>) -> Result<PyValue, PickleError> {
    Ok(PyValue::Dict(Arc::new(PyMapping {
        entries: pairs_from_values(items)?,
    })))
}

fn pairs_from_values(items: Vec<PyValue>) -> Result<Vec<(PyValue, PyValue)>, PickleError> {
    if !items.len().is_multiple_of(2) {
        return Err(PickleError::Malformed("odd number of dict items"));
    }
    let mut entries = Vec::with_capacity(items.len() / 2);
    let mut iterator = items.into_iter();
    while let (Some(key), Some(value)) = (iterator.next(), iterator.next()) {
        entries.push((key, value));
    }
    Ok(entries)
}

/// `raw-unicode-escape` decoding, for protocol-0 `UNICODE` strings.
fn raw_unicode_escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'\\'
            && position + 5 < bytes.len()
            && bytes[position + 1] == b'u'
            && let Ok(hex) = std::str::from_utf8(&bytes[position + 2..position + 6])
            && let Ok(code) = u32::from_str_radix(hex, 16)
        {
            match char::from_u32(code) {
                Some(ch) => {
                    let mut buffer = [0_u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                }
                None => {
                    // A lone surrogate: WTF-8 encode it.
                    let code = code as u16;
                    out.extend_from_slice(
                        plane_serde::pyjson::PyStr::from_wtf8(wtf8_surrogate(code))
                            .unwrap_or_default()
                            .as_wtf8(),
                    );
                }
            }
            position += 6;
            continue;
        }
        out.push(bytes[position]);
        position += 1;
    }
    out
}

/// WTF-8 bytes for one code point in the Basic Multilingual Plane.
fn wtf8_surrogate(code: u16) -> Vec<u8> {
    if code < 0x80 {
        return vec![code as u8];
    }
    if code < 0x800 {
        return vec![0xc0 | (code >> 6) as u8, 0x80 | (code & 0x3f) as u8];
    }
    vec![
        0xe0 | (code >> 12) as u8,
        0x80 | ((code >> 6) & 0x3f) as u8,
        0x80 | (code & 0x3f) as u8,
    ]
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

    fn u32(&mut self, what: &'static str) -> Result<usize, PickleError> {
        let bytes = self.take(4, what)?;
        let mut array = [0_u8; 4];
        array.copy_from_slice(bytes);
        Ok(u32::from_le_bytes(array) as usize)
    }

    fn u64(&mut self, what: &'static str) -> Result<usize, PickleError> {
        let bytes = self.take(8, what)?;
        let mut array = [0_u8; 8];
        array.copy_from_slice(bytes);
        usize::try_from(u64::from_le_bytes(array))
            .map_err(|_| PickleError::Malformed("length out of range"))
    }

    fn skip(&mut self, count: usize, what: &'static str) -> Result<(), PickleError> {
        self.take(count, what).map(|_| ())
    }

    fn line(&mut self, what: &'static str) -> Result<&str, PickleError> {
        let line = self.line_bytes(what)?;
        std::str::from_utf8(line).map_err(|_| PickleError::Malformed(what))
    }

    fn line_bytes(&mut self, what: &'static str) -> Result<&[u8], PickleError> {
        let rest = &self.bytes[self.position..];
        let Some(end) = rest.iter().position(|byte| *byte == b'\n') else {
            return Err(PickleError::Truncated(what));
        };
        let line = &rest[..end];
        self.position += end + 1;
        Ok(line)
    }
}
