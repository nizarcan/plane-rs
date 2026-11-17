# Vendored: zxcvbn 3.1.1 (MIT)

Upstream: <https://github.com/shssoichiro/zxcvbn-rs>, crate `zxcvbn` 3.1.1
(`~/.cargo/registry/src/.../zxcvbn-3.1.1`), licence in `LICENSE` (MIT). The
sources are copied verbatim except for the change below. This directory is
excluded from the Cargo workspace (`apps/server/Cargo.toml`, `exclude`) and
wired in with `[patch.crates-io]`; `crates/plane-auth` keeps the normal
`zxcvbn = "3.1.1"` dependency.

## The changes

`src/lib.rs`, in `zxcvbn()`:

```diff
-        // Only evaluate the first 100 characters of the input.
-        // This prevents potential DoS attacks from sending extremely long input strings.
-        let password = password.chars().take(100).collect::<String>();
+        // Only evaluate the first 128 characters of the input.
+        // ...
+        let password = password.chars().take(128).collect::<String>();
```

`src/matching/mod.rs`, at the end of `L33tMatch::get_matches`:

```diff
         matches
             .into_iter()
-            .filter(|x| !x.token.is_empty())
+            .filter(|x| x.token.chars().count() > 1)
             .collect()
```

## Why

`apps/api/plane/authentication/utils/password.py` scores the first and last
`ZXCVBN_MAX_LENGTH = 128` characters with Python `zxcvbn==4.4.28` and requires
`score >= 3`. Python's `zxcvbn()` has no length cap (`zxcvbn/matching.py` and
`scoring.py` process the whole string), while upstream 3.1.1 silently scores
only the first 100 characters, so it disagreed with Python on 33 of the 262
recorded windows longer than 100 in `tools/auth/zxcvbn_corpus.json`.

With the cap at 128 (the window length `is_password_strong` passes, so the cap
is never reached from that call site), the crate agrees with Python on every
recorded window, including the 196 new passwords added for the 101–128
character shapes. If another caller passes a longer password, the crate still
truncates at 128; Plane's policy never does that.

The l33t filter is a second, independent divergence the first one exposed.
Python keeps only l33t dictionary matches longer than one character
(`zxcvbn/matching.py:244`); upstream 3.1.1 accepted one-character matches, so
`"1"` also matched the word `"i"`. Inside `repeat_match`, Python scores the
base string separately: `"1" * 128` is a repeat of `"1"`, whose base guesses
are 12 (11 brute-force guesses plus the additive 1); the crate's bogus
one-character match made the base guesses 5, so the whole 128-character
window scored 0 instead of 1. After the fix all 1,484 recorded windows match
Python's score exactly.

## Keeping it small

When `zxcvbn` is upgraded upstream: copy the new source here, re-apply the
two changes (or drop them if upstream fixed them), update the version in
`Cargo.toml` and this file, re-run
`cargo test -p plane-auth --test zxcvbn_corpus`, and if upstream no longer
truncates and filters the same way, drop the vendor directory and the patch
instead.
