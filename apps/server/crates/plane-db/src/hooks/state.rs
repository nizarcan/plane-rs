// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H19 — `State.save` (`plane/db/models/state.py:117-126`).
//!
//! ```python
//! self.slug = slugify(self.name)
//! if self._state.adding:
//!     last_id = State.objects.filter(project=self.project).aggregate(largest=Max("sequence"))["largest"]
//!     if last_id is not None:
//!         self.sequence = last_id + 15000
//! return super().save()
//! ```
//!
//! The slug is recomputed on **every** save, including the full save a soft
//! delete performs. On create, the sequence is `MAX(sequence) + 15000` when the
//! project has any live non-triage state; otherwise the supplied value (or the
//! model default 65535) is kept. `State.objects` is `StateManager`, so the
//! aggregate hides soft-deleted and `group = 'triage'` rows.

use unicode_normalization::UnicodeNormalization;

/// H19's slug half: `django.template.defaultfilters.slugify(name)`
/// (`django/utils/text.py:448-465`).
#[must_use]
pub fn slugify(value: &str) -> String {
    // `unicodedata.normalize('NFKD', value).encode('ascii', 'ignore')` then
    // `.strip().lower()`; the two steps are commutative here.
    let ascii: String = value
        .nfkd()
        .filter(char::is_ascii)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    // `re.sub(r'[^\w\s-]', '', value)`; after the ASCII step `\w` is
    // `[A-Za-z0-9_]` and `\s` is ASCII whitespace.
    let filtered: String = ascii
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || *character == '_'
                || character.is_ascii_whitespace()
                || *character == '-'
        })
        .collect();
    // `re.sub(r'[-\s]+', '-', value).strip('-_')`.
    let mut slug = String::with_capacity(filtered.len());
    let mut separator = false;
    for character in filtered.chars() {
        if character == '-' || character.is_ascii_whitespace() {
            if !separator {
                slug.push('-');
                separator = true;
            }
        } else {
            slug.push(character);
            separator = false;
        }
    }
    slug.trim_matches(|character| character == '-' || character == '_')
        .to_owned()
}

/// H19: the slug and sequence a `save()` writes.
#[derive(Debug, Clone, PartialEq)]
pub struct StateSave {
    pub slug: String,
    pub sequence: f64,
}

/// H19. `max_sequence` is the `MAX(sequence)` over `State.objects` for the
/// project, or `None` when the aggregate returned NULL.
#[must_use]
pub fn before_save(
    name: &str,
    adding: bool,
    sequence: f64,
    max_sequence: Option<f64>,
) -> StateSave {
    StateSave {
        slug: slugify(name),
        sequence: if adding {
            max_sequence.map_or(sequence, |largest| largest + 15_000.0)
        } else {
            sequence
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn django_slugify_ascii_unicode_and_separators() {
        assert_eq!(slugify("In Progress"), "in-progress");
        assert_eq!(slugify("  Backlog  "), "backlog");
        assert_eq!(slugify("Café & Crème"), "cafe-creme");
        assert_eq!(slugify("a -- b__c"), "a-b__c");
        assert_eq!(slugify("--x-"), "x");
        assert_eq!(slugify("你好"), "");
        assert_eq!(slugify("Todo!"), "todo");
    }

    #[test]
    fn h19_sequence_only_changes_on_create() {
        let created = before_save("Todo", true, 12.0, Some(15000.0));
        assert_eq!(created.sequence, 30000.0);
        let first = before_save("Todo", true, 12.0, None);
        assert_eq!(first.sequence, 12.0);
        let updated = before_save("Todo", false, 12.0, Some(15000.0));
        assert_eq!(updated.sequence, 12.0);
        assert_eq!(updated.slug, "todo");
    }
}
