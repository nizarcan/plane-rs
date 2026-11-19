# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# `csrf_failure.html` is a byte-identical copy of
# `apps/api/templates/csrf_failure.html` at the commit in `UPSTREAM.toml`
# (the oracle). Django renders it for a CSRF rejection with HTTP 200; the Rust
# middleware embeds it with `include_str!`. The HTML has no licence header
# because a comment would change the rendered bytes the contract compares.
#
# `tools/boot/static_files.py` covers the other embedded Django assets; this
# one is not a static file, so the copy is checked by the CSRF scenario and by
# `bins/plane-api/src/middleware/csrf.rs`'s unit tests. When the pin moves,
# refresh it from the target oracle and re-run that scenario.
