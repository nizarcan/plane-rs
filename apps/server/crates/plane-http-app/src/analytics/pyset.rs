// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! CPython 3.12's `set` iteration order for the five `project-stats` fields.
//!
//! `ProjectStatsEndpoint.get` intersects two sets and splats the result into
//! `values("id", *requested_fields)`, so each row's JSON key order is the
//! *iteration order of a Python set* (`plane/app/views/analytic/base.py:416-486`):
//!
//! ```python
//! requested_fields = set(filter(None, fields)) & valid_fields
//! projects = projects.annotate(**annotations).values("id", *requested_fields)
//! ```
//!
//! That order is not a contract — it moves with `PYTHONHASHSEED` and with the
//! CPython version — but the differential harness compares object key order
//! byte for byte and the scenario pins `PYTHONHASHSEED=0`, so the port has to
//! reproduce one particular layout. `DECISIONS.md` D16 accepts a *stable* order
//! for set-derived **values**; it does not cover object keys, so this module
//! reproduces CPython's instead of inventing one.
//!
//! The pieces, from `Objects/setobject.c` (CPython 3.12):
//!
//! * a new set has an 8-slot table; `set_add_entry` starts at `hash & mask` and
//!   then walks `LINEAR_PROBES = 9` neighbours *only when the window fits*
//!   (`i + LINEAR_PROBES <= mask`, which is never true in an 8-slot table),
//!   otherwise it perturbs the index: `i = (i * 5 + 1 + (hash >> 5)) & mask`;
//! * the table is resized when `fill * 5 >= mask * 3`, to a power of two above
//!   `used * 4`;
//! * `set.__and__` swaps the operands when `len(other) > len(so)`, then
//!   iterates `other` and re-inserts the members `so` contains into a **fresh**
//!   set — so the result's order is that new table's layout, built by inserting
//!   `other`'s iteration order. With `so = set(fields)` and
//!   `other = valid_fields`, `other` is the literal on a size tie.
//!
//! The hashes are CPython 3.12.13's `hash(name)` with `PYTHONHASHSEED=0` (which
//! zeroes the hash secret). `field_order_agrees_with_cpython` pins all 151
//! subset/order combinations against orders recorded from the interpreter.
//!
//! Known approximation: an **invalid** field name in `?fields=` still occupies
//! a slot in the intermediate `set(...)`, which this module cannot reproduce
//! (any string can be hashed). The valid names' order can therefore differ from
//! Django's when the request mixes invalid names into a list of ≤ 5 names. The
//! web app only ever sends valid names.

/// `VALID_FIELDS` (`plane/app/views/analytic/base.py:416-423`), in the order
/// of the set literal in the source — which is the insertion order of the set
/// used when `fields=` is absent.
pub(crate) const NAMES: [&str; 5] = [
    "total_issues",
    "completed_issues",
    "total_members",
    "total_cycles",
    "total_modules",
];

/// `list(valid_fields)` under `PYTHONHASHSEED=0`, CPython 3.12.13.
///
/// The literal is compiled to a `frozenset` constant that `SET_UPDATE` merges
/// into an empty set, and `set_merge` copies the frozenset's table verbatim —
/// so its layout is the *compiler's* build order, not an insertion sequence
/// this module can replay. It is recorded here instead, by index into
/// [`NAMES`] (see `field_order_agrees_with_cpython`).
const LITERAL_ORDER: [usize; 5] = [2, 3, 1, 4, 0];

/// `hash(name)` under `PYTHONHASHSEED=0`, CPython 3.12.13, as `u64`.
const HASHES: [u64; 5] = [
    8_699_906_039_509_072_808,
    5_571_669_806_763_436_904,
    1_109_046_315_128_771_393,
    16_712_010_722_899_679_634,
    13_879_440_442_887_294_630,
];

/// `LINEAR_PROBES` (`Objects/setobject.c`).
const LINEAR_PROBES: usize = 9;
/// `PERTURB_SHIFT`.
const PERTURB_SHIFT: u32 = 5;
/// `PySet_MINSIZE`.
const MIN_SIZE: usize = 8;

/// The subset of `set` this module needs: insert, resize, iterate.
struct SmallSet {
    /// Slot → index into [`NAMES`].
    slots: Vec<Option<usize>>,
    fill: usize,
    used: usize,
}

impl SmallSet {
    fn new() -> Self {
        Self {
            slots: vec![None; MIN_SIZE],
            fill: 0,
            used: 0,
        }
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// `set_add_entry` for a key that is not present yet.
    fn insert(&mut self, index: usize) {
        if self.slots.contains(&Some(index)) {
            return;
        }
        let hash = HASHES[index];
        let mask = self.mask();
        let mut i = (hash as usize) & mask;
        let mut perturb = hash;
        loop {
            // `probes = (i + LINEAR_PROBES <= mask) ? LINEAR_PROBES : 0;`
            let probes = if i + LINEAR_PROBES <= mask {
                LINEAR_PROBES
            } else {
                0
            };
            for slot in i..=i + probes {
                if self.slots[slot].is_none() {
                    self.slots[slot] = Some(index);
                    self.fill += 1;
                    self.used += 1;
                    // `if ((size_t)so->fill*5 < mask*3) return 0;`
                    if self.fill * 5 >= mask * 3 {
                        self.resize(self.used * 4);
                    }
                    return;
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

    /// `set_table_resize`: probe the old table in slot order into a bigger one.
    fn resize(&mut self, minused: usize) {
        let mut newsize = MIN_SIZE;
        while newsize <= minused {
            newsize <<= 1;
        }
        let old = std::mem::replace(&mut self.slots, vec![None; newsize]);
        for index in old.into_iter().flatten() {
            self.insert_clean(index);
        }
        self.fill = self.slots.iter().filter(|slot| slot.is_some()).count();
        self.used = self.fill;
    }

    /// `set_insert_clean`: the same probe, without the duplicate and resize
    /// checks (the caller recomputes `fill`/`used`).
    fn insert_clean(&mut self, index: usize) {
        let hash = HASHES[index];
        let mask = self.mask();
        let mut i = (hash as usize) & mask;
        let mut perturb = hash;
        loop {
            let probes = if i + LINEAR_PROBES <= mask {
                LINEAR_PROBES
            } else {
                0
            };
            for slot in i..=i + probes {
                if self.slots[slot].is_none() {
                    self.slots[slot] = Some(index);
                    return;
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

    fn contains(&self, index: usize) -> bool {
        self.slots.contains(&Some(index))
    }

    /// Slot order, which is CPython's iteration order.
    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.slots.iter().filter_map(|slot| *slot)
    }
}

/// `set(filter(None, fields)) & valid_fields`, in CPython's iteration order.
///
/// `None` (the parameter absent) is `valid_fields` itself.
pub(crate) fn field_order(fields: Option<&str>) -> Vec<&'static str> {
    let mut valid = SmallSet::new();
    for index in 0..NAMES.len() {
        valid.insert(index);
    }
    let literal =
        || -> Vec<&'static str> { LITERAL_ORDER.iter().map(|index| NAMES[*index]).collect() };
    let Some(fields) = fields else {
        return literal();
    };

    let mut requested = SmallSet::new();
    for name in fields.split(',').filter(|name| !name.is_empty()) {
        if let Some(index) = NAMES.iter().position(|candidate| *candidate == name) {
            requested.insert(index);
        }
    }

    // `set.__and__` → `set_intersection(so=requested, other=valid)`:
    // `if (PySet_GET_SIZE(other) > PySet_GET_SIZE(so)) swap()`, then it
    // iterates **other**, which on a size tie is `valid_fields`.
    if valid.used > requested.used {
        // `other` is `set(fields)`: replay its (emulated) iteration order.
        let mut result = SmallSet::new();
        for index in requested.iter() {
            if valid.contains(index) {
                result.insert(index);
            }
        }
        if result.used == 0 {
            // `if not requested_fields: requested_fields = valid_fields`.
            return literal();
        }
        return result.iter().map(|index| NAMES[index]).collect();
    }
    // `other` is `valid_fields`: insert its recorded order into a fresh set,
    // whose own layout is what the caller then iterates.
    let mut result = SmallSet::new();
    for index in LITERAL_ORDER {
        if requested.contains(index) {
            result.insert(index);
        }
    }
    if result.used == 0 {
        return literal();
    }
    result.iter().map(|index| NAMES[index]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Orders recorded from CPython 3.12.13 with `PYTHONHASHSEED=0`:
    /// `list(set(filter(None, fields.split(","))) & valid_fields)`.
    #[test]
    fn field_order_agrees_with_cpython() {
        let cases: [(&str, &[&str]); 14] = [
            ("total_issues", &["total_issues"]),
            ("total_issues,", &["total_issues"]),
            (
                "total_issues,completed_issues",
                &["total_issues", "completed_issues"],
            ),
            (
                "completed_issues,total_issues",
                &["completed_issues", "total_issues"],
            ),
            (
                "completed_issues,total_members",
                &["completed_issues", "total_members"],
            ),
            (
                "total_members,completed_issues",
                &["completed_issues", "total_members"],
            ),
            (
                "total_issues,completed_issues,total_members",
                &["total_issues", "total_members", "completed_issues"],
            ),
            (
                "total_issues,completed_issues,total_cycles",
                &["total_issues", "total_cycles", "completed_issues"],
            ),
            (
                "total_issues,total_members,total_cycles",
                &["total_issues", "total_members", "total_cycles"],
            ),
            (
                "total_issues,completed_issues,total_members,total_modules",
                &[
                    "total_issues",
                    "total_members",
                    "completed_issues",
                    "total_modules",
                ],
            ),
            (
                "total_issues,completed_issues,total_cycles,total_modules",
                &[
                    "total_issues",
                    "total_cycles",
                    "completed_issues",
                    "total_modules",
                ],
            ),
            // Listing every field explicitly is *not* the literal's order:
            // `set.__and__` iterates the 5-element `set(...)` instead.
            (
                "total_issues,completed_issues,total_members,total_cycles,total_modules",
                &[
                    "total_members",
                    "total_modules",
                    "completed_issues",
                    "total_issues",
                    "total_cycles",
                ],
            ),
            // No `fields` parameter is the `valid_fields` set literal.
            (
                "",
                &[
                    "total_members",
                    "total_cycles",
                    "completed_issues",
                    "total_modules",
                    "total_issues",
                ],
            ),
            (
                "bogus",
                &[
                    "total_members",
                    "total_cycles",
                    "completed_issues",
                    "total_modules",
                    "total_issues",
                ],
            ),
        ];
        for (fields, expected) in cases {
            let actual = if fields.is_empty() {
                field_order(None)
            } else {
                field_order(Some(fields))
            };
            assert_eq!(actual, expected, "fields={fields:?}");
        }
    }

    #[test]
    fn every_single_field_is_its_own_order() {
        for (index, name) in NAMES.iter().enumerate() {
            assert_eq!(field_order(Some(name)), vec![*name], "fields={name}");
            assert_eq!(
                field_order(Some("bogus,total_issues")),
                vec![NAMES[0]],
                "an invalid name is dropped ({index})"
            );
        }
    }
}
