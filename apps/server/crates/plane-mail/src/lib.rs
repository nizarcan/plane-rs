// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django-compatible e-mail machinery for the Rust backend.
//!
//! Three pieces, all of them ported from `apps/api`:
//!
//! * [`transport`] — `django.core.mail`'s `EmailMultiAlternatives` bytes and
//!   the smtplib-faithful SMTP session (D23). Moved here from
//!   `plane-http-app::instances::mail`; `plane-http-app` re-exports it so the
//!   existing route did not change.
//! * [`plain_text`] — `plane.utils.email.generate_plain_text_from_html`
//!   (`plane/utils/email.py:36-63`), which every HTML e-mail task uses to build
//!   the `text/plain` alternative.
//! * [`template`] — Django's `render_to_string("emails/...")` for the e-mail
//!   templates, rendered by minijinja from checked-in `.j2` copies (D12).
//!
//! [`EmailConfiguration::load`] is the `get_email_configuration()` half
//! (`plane/license/utils/instance_value.py:42-59`).

pub mod plain_text;
pub mod template;
pub mod transport;

pub use plain_text::generate_plain_text_from_html;
pub use template::{TemplateError, render, render_with_environment};
pub use transport::{
    DEFAULT_FROM_EMAIL, EmailConfiguration, MailError, Message, default_date, default_message_id,
    dns_name, quoteaddr, sanitize_address, send,
};
