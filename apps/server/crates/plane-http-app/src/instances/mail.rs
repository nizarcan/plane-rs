// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Re-export of [`plane_mail::transport`].
//!
//! The SMTP session and the `EmailMultiAlternatives` byte writer moved to
//! `crates/plane-mail` in Phase 8 P0 so task handlers share them with this
//! endpoint (`plans/CUTOVER.md` §6). The module path stays so the handler,
//! its callers and the golden test's historical import did not change.
//! The golden test itself moved to `crates/plane-mail/tests/`.

pub use plane_mail::transport::*;
