// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Lua scripts Plane runs through `redis-py`'s `eval()`, byte for byte.
//!
//! `redis-py` sends the script source and its arguments as one `EVAL`
//! command; there is no parameter binding and no script cache management, so
//! the effect of a script is the effect of its text. This module holds the
//! text, and [`Redis::eval`](crate::Redis::eval) issues the command.

/// One argument to [`Redis::eval`](crate::Redis::eval), encoded the way
/// `redis-py` encodes a command argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalArg<'a> {
    /// A Python `int`: its decimal text.
    Int(i64),
    /// A Python `str`: its UTF-8 bytes.
    Str(&'a str),
    /// A Python `bytes`.
    Bytes(&'a [u8]),
}

/// `MagicCodeProvider._INCREMENT_COUNTER_SCRIPT`
/// (`plane/authentication/provider/credentials/magic_code.py:40-46`).
///
/// `ARGV[1]` is the TTL in **seconds**. `INCR` keeps an existing TTL, so the
/// `EXPIRE` on the first call is the only one that ever runs and the counter
/// expires together with the key it counts — the property that makes the
/// magic-code caps impossible to reset by waiting out a single token.
///
/// The text is the Python interpolation-free literal, with its single spaces
/// preserved: Redis caches a script by the SHA1 of its source, and keeping the
/// text identical means the two backends share one cached entry per node.
pub const INCR_EXPIRE_ON_FIRST: &str = concat!(
    "local count = redis.call(\"INCR\", KEYS[1]) ",
    "if count == 1 then ",
    "    redis.call(\"EXPIRE\", KEYS[1], tonumber(ARGV[1])) ",
    "end ",
    "return count",
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_script_is_the_python_text() {
        // Concatenated exactly as the Python literal is, four-space indent
        // included (`concat!` keeps it; a `\\` continuation would not).
        const EXPECTED: &str = concat!(
            "local count = redis.call(\"INCR\", KEYS[1]) ",
            "if count == 1 then ",
            "    redis.call(\"EXPIRE\", KEYS[1], tonumber(ARGV[1])) ",
            "end ",
            "return count",
        );
        assert_eq!(INCR_EXPIRE_ON_FIRST, EXPECTED);
    }
}
