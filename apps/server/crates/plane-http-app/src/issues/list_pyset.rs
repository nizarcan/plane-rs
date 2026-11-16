// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! CPython's `list(set(...))` order for `str` elements.
//!
//! `GroupedOffsetPaginator.__query_multi_grouper` builds `label_ids`,
//! `assignee_ids` and `module_ids` from a `set` of `str(uuid)` values
//! (`plane/utils/paginator.py:601-618`), so the array order is CPython's set
//! iteration order. `crates/plane-query` sorts those arrays and normalises the
//! difference in its golden corpus
//! (`crates/plane-query/README.md`, "Documented normalisations" 1); the
//! differential harness compares the bytes, so the port has to reproduce the
//! order.
//!
//! Two facts make that possible:
//!
//! * the scenarios pin `PYTHONHASHSEED=0`, which disables hash randomisation:
//!   CPython zeroes `_Py_HashSecret` (`Python/bootstrap_hash.c`,
//!   `_Py_HashRandomization_Init`), so `hash(str)` is SipHash-1-3 with a zero
//!   key (`Python/pyhash.c`);
//! * `set` is an open-addressing table whose layout is fully determined by the
//!   hashes and the insertion order (`Objects/setobject.c` in CPython 3.12:
//!   `set_add_entry`, `set_table_resize`, `set_insert_clean`).
//!
//! The implementation is a direct transcription of those three functions.
//!
//! ## Where the insertion order comes from
//!
//! `set.add` runs in page order, and the page order for rows that share
//! `created_at` is whatever Postgres emits; `fix_multi_group_arrays` re-runs
//! the page statement and rebuilds the lists in that order.

/// `_Py_HASH_BITS`: the hash is a `Py_hash_t` (signed 64-bit).
const MASK64: u64 = u64::MAX;

/// `PERTURB_SHIFT`.
const PERTURB_SHIFT: u32 = 5;

/// `LINEAR_PROBES` (`Objects/setobject.c`).
const LINEAR_PROBES: usize = 9;

/// `PySet_MINSIZE`.
const SET_MINSIZE: usize = 8;

/// `list(set(values))`, built in `values` order.
#[must_use]
pub fn python_set_order(values: &[String]) -> Vec<String> {
    let mut set = PySet::new();
    for value in values {
        set.add(value.clone());
    }
    set.into_order()
}

/// One occupied slot: the element's hash and its index in `keys`.
#[derive(Clone, Copy, Debug)]
struct Slot {
    hash: u64,
    key: usize,
}

#[derive(Debug)]
struct PySet {
    table: Vec<Option<Slot>>,
    fill: usize,
    keys: Vec<String>,
}

impl PySet {
    fn new() -> Self {
        Self {
            table: vec![None; SET_MINSIZE],
            fill: 0,
            keys: Vec::new(),
        }
    }

    fn mask(&self) -> usize {
        self.table.len() - 1
    }

    fn add(&mut self, key: String) {
        let hash = py_hash_str(&key);
        // `set_add_entry` returns without touching the table for a duplicate.
        if self.find(hash, &key).is_some() {
            return;
        }
        let index = self.keys.len();
        self.keys.push(key);
        self.insert_entry(Slot { hash, key: index });
        // `found_unused`: the resize runs *after* the insertion.
        if self.fill * 5 >= self.mask() * 3 {
            let used = self.keys.len();
            let minused = if used > 50_000 { used * 2 } else { used * 4 };
            self.resize(minused.max(SET_MINSIZE));
        }
    }

    fn find(&self, hash: u64, key: &str) -> Option<usize> {
        let mask = self.mask();
        let mut i = (hash as usize) & mask;
        let mut perturb = hash;
        loop {
            match self.table[i] {
                None => return None,
                Some(slot) if slot.hash == hash && self.keys[slot.key] == key => {
                    return Some(slot.key);
                }
                Some(_) => {}
            }
            if i + LINEAR_PROBES <= mask {
                for offset in 1..=LINEAR_PROBES {
                    let probe = i + offset;
                    match self.table[probe] {
                        None => return None,
                        Some(slot) if slot.hash == hash && self.keys[slot.key] == key => {
                            return Some(slot.key);
                        }
                        Some(_) => {}
                    }
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i
                .wrapping_mul(5)
                .wrapping_add(1)
                .wrapping_add(perturb as usize))
                & mask;
        }
    }

    /// `set_add_entry` on a table with a free slot guaranteed to exist.
    fn insert_entry(&mut self, slot: Slot) {
        let mask = self.mask();
        let mut i = (slot.hash as usize) & mask;
        let mut perturb = slot.hash;
        loop {
            if self.table[i].is_none() {
                self.table[i] = Some(slot);
                self.fill += 1;
                return;
            }
            if i + LINEAR_PROBES <= mask {
                for offset in 1..=LINEAR_PROBES {
                    if self.table[i + offset].is_none() {
                        self.table[i + offset] = Some(slot);
                        self.fill += 1;
                        return;
                    }
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i
                .wrapping_mul(5)
                .wrapping_add(1)
                .wrapping_add(perturb as usize))
                & mask;
        }
    }

    fn resize(&mut self, minused: usize) {
        let mut size = SET_MINSIZE;
        while size <= minused {
            size <<= 1;
        }
        let mut table = vec![None; size];
        let old = std::mem::take(&mut self.table);
        self.fill = 0;
        for slot in old.into_iter().flatten() {
            let mask = size - 1;
            let mut i = (slot.hash as usize) & mask;
            let mut perturb = slot.hash;
            loop {
                if table[i].is_none() {
                    table[i] = Some(slot);
                    self.fill += 1;
                    break;
                }
                if i + LINEAR_PROBES <= mask {
                    let mut placed = false;
                    for offset in 1..=LINEAR_PROBES {
                        if table[i + offset].is_none() {
                            table[i + offset] = Some(slot);
                            self.fill += 1;
                            placed = true;
                            break;
                        }
                    }
                    if placed {
                        break;
                    }
                }
                perturb >>= PERTURB_SHIFT;
                i = (i
                    .wrapping_mul(5)
                    .wrapping_add(1)
                    .wrapping_add(perturb as usize))
                    & mask;
            }
        }
        self.table = table;
    }

    fn into_order(self) -> Vec<String> {
        // `set_next` scans the table in index order.
        self.table
            .into_iter()
            .flatten()
            .map(|slot| self.keys[slot.key].clone())
            .collect()
    }
}

/// `_Py_HashBytes` → SipHash-1-3 with a zero key.
///
/// CPython maps a result of `-1` to `-2`; as an unsigned pattern that is
/// `u64::MAX` → `u64::MAX - 1`.
#[must_use]
pub fn py_hash_str(value: &str) -> u64 {
    let hash = siphash13(0, 0, value.as_bytes());
    if hash == MASK64 { MASK64 - 1 } else { hash }
}

fn siphash13(k0: u64, k1: u64, data: &[u8]) -> u64 {
    let mut v0: u64 = 0x736f_6d65_7073_6575 ^ k0;
    let mut v1: u64 = 0x646f_7261_6e64_6f6d ^ k1;
    let mut v2: u64 = 0x6c79_6765_6e65_7261 ^ k0;
    let mut v3: u64 = 0x7465_6462_7974_6573 ^ k1;

    let mut offset = 0;
    while offset + 8 <= data.len() {
        let m = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap_or_default());
        v3 ^= m;
        single_round(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= m;
        offset += 8;
    }

    let remainder = &data[offset..];
    let mut b = (data.len() as u64 & 0xff) << 56;
    for (index, byte) in remainder.iter().enumerate() {
        b |= u64::from(*byte) << (8 * index);
    }

    v3 ^= b;
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= b;
    v2 ^= 0xff;
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    single_round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^ v1 ^ v2 ^ v3
}

/// CPython's `SINGLE_ROUND` (`Python/pyhash.c`), which is **not** the reference
/// SipRound: the `HALF_ROUND` macro xor-s the rotated value into the *sum*
/// register (`b = ROTATE(b, s) ^ a`) instead of into the rotated one.
fn single_round(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    // HALF_ROUND(v0, v1, v2, v3, 13, 16)
    *v0 = v0.wrapping_add(*v1);
    *v2 = v2.wrapping_add(*v3);
    *v1 = v1.rotate_left(13) ^ *v0;
    *v3 = v3.rotate_left(16) ^ *v2;
    *v0 = v0.rotate_left(32);
    // HALF_ROUND(v2, v1, v0, v3, 17, 21)
    *v2 = v2.wrapping_add(*v1);
    *v0 = v0.wrapping_add(*v3);
    *v1 = v1.rotate_left(17) ^ *v2;
    *v3 = v3.rotate_left(21) ^ *v0;
    *v2 = v2.rotate_left(32);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(words: &[&str]) -> Vec<String> {
        python_set_order(
            &words
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    /// `PYTHONHASHSEED=0 python3 -c 's=set(); [s.add(w) for w in
    /// ["a","bb","ccc","dddd","eeeee"]]; print(list(s))'`
    #[test]
    fn short_strings_match_cpython() {
        assert_eq!(
            build(&["a", "bb", "ccc", "dddd", "eeeee"]),
            vec!["dddd", "eeeee", "a", "bb", "ccc"]
        );
    }

    #[test]
    fn the_str_hash_matches_cpython() {
        // `PYTHONHASHSEED=0 python3 -c 'print(hash("a"), hash("bb"))'`
        assert_eq!(py_hash_str("a") as i64, 4_644_417_185_603_328_019);
        assert_eq!(py_hash_str("bb") as i64, -4_192_514_204_586_706_540);
    }

    /// The two fixture assignee ids whose slots collide: `set` keeps the
    /// insertion order.
    #[test]
    fn colliding_uuids_keep_insertion_order() {
        let first = "f1d4b6e4-0f7b-4fec-8130-208fa8cecc93";
        let second = "bc437914-c250-4c9f-ac69-85003fdf6205";
        assert_eq!(build(&[first, second]), vec![first, second]);
        assert_eq!(build(&[second, first]), vec![second, first]);
    }

    #[test]
    fn non_colliding_uuids_ignore_insertion_order() {
        let admin = "d44ae570-3de4-4413-af08-f686bf5b7235";
        let member = "78100240-9cfd-4782-8627-25ab039c6e9a";
        let member2 = "bc437914-c250-4c9f-ac69-85003fdf6205";
        assert_eq!(
            build(&[admin, member, member2]),
            build(&[member2, member, admin])
        );
        assert_eq!(
            build(&[admin, member, member2]),
            vec![member2, admin, member]
        );
    }

    #[test]
    fn duplicates_collapse_to_one_entry() {
        assert_eq!(build(&["x", "x"]), vec!["x"]);
    }
}
