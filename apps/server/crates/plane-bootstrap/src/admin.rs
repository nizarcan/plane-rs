// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-admin`'s copies of three Django management commands
//! (`plans/CUTOVER.md` §3): `reset_password`, `activate_user` and `test_email`.
//!
//! Behaviour and output are the Python commands':
//!
//! * branches that `self.stderr.write(...)` and `return` print to stderr and
//!   exit **0** (`reset_password.py:28-52`);
//! * `raise CommandError(...)` prints `CommandError: <message>` to stderr and
//!   exits **1** (`activate_user.py:25-32`; `reset_password.py:54-55`);
//! * success lines are `self.stdout.write(self.style.SUCCESS(...))`, which is
//!   plain text on a non-tty (and carries ANSI codes on a tty — the port does
//!   not colourise, a cosmetic difference);
//! * `test-email`'s delivery failure is `self.stdout.write(self.style.ERROR(...))`
//!   on **stdout**, and the command exits 0 (`test_email.py:66-67`).
//!
//! `getpass.getpass` reads from the terminal without echo. No echo suppression
//! is implemented: that needs a terminal-io crate the workspace does not pin,
//! and adding one is not worth it for an operator command. The prompts and the
//! read are the same; a piped run (the comparison harness) behaves identically,
//! because Python also falls back to a plain read when stdin is not a tty.

use std::io::Write;

use deadpool_postgres::Client;
use plane_auth::auth_flow::is_password_strong_async;
use plane_core::config::Settings;
use plane_mail::{EmailConfiguration, Message, generate_plain_text_from_html, render};

use crate::{BootError, Result};

/// The outcome `plane-admin` maps to an exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminError {
    /// `CommandError`: `manage.py` writes `CommandError: <message>` to stderr
    /// and exits 1.
    CommandError(String),
}

/// `reset_password <email>` (`plane/db/management/commands/reset_password.py`).
///
/// # Errors
///
/// [`AdminError::CommandError`] when zxcvbn rejects the password
/// (`reset_password.py:54-55`); a database error is [`BootError`].
pub async fn reset_password(
    client: &Client,
    email: &str,
) -> Result<std::result::Result<(), AdminError>> {
    if email.is_empty() {
        eprintln!("Error: Email is required");
        return Ok(Ok(()));
    }
    let user = client
        .query_opt("SELECT id FROM users WHERE email = $1 LIMIT 1", &[&email])
        .await?;
    let Some(row) = user else {
        eprintln!("Error: User with {email} does not exists");
        return Ok(Ok(()));
    };
    let id: uuid::Uuid = row.get(0);

    let password = prompt("Password: ");
    let confirmation = prompt("Password (again): ");
    if password != confirmation {
        eprintln!("Error: Your passwords didn't match.");
        return Ok(Ok(()));
    }
    if password.trim().is_empty() {
        eprintln!("Error: Blank passwords aren't allowed.");
        return Ok(Ok(()));
    }
    let strong = is_password_strong_async(&password)
        .await
        .map_err(|error| BootError::Message(format!("password strength check failed: {error}")))?;
    if !strong {
        return Ok(Err(AdminError::CommandError(
            "Password is too common please set a complex password".to_string(),
        )));
    }

    // `user.set_password(password)` + `is_password_autoset = False` + `save()`.
    let hash = plane_auth::password::make_password(Some(&password));
    client
        .execute(
            "UPDATE users SET password = $2, is_password_autoset = false, \
             updated_at = now() WHERE id = $1",
            &[&id, &hash],
        )
        .await?;
    println!("User password updated successfully");
    Ok(Ok(()))
}

/// `activate_user <email>` (`activate_user.py:19-38`).
///
/// # Errors
///
/// [`AdminError::CommandError`] for a missing email or user, as the Python
/// `CommandError`s.
pub async fn activate_user(
    client: &Client,
    email: &str,
) -> Result<std::result::Result<(), AdminError>> {
    if email.is_empty() {
        return Ok(Err(AdminError::CommandError(
            "Error: Email is required".to_string(),
        )));
    }
    let user = client
        .query_opt("SELECT id FROM users WHERE email = $1 LIMIT 1", &[&email])
        .await?;
    let Some(row) = user else {
        return Ok(Err(AdminError::CommandError(format!(
            "Error: User with {email} does not exists"
        ))));
    };
    let id: uuid::Uuid = row.get(0);
    client
        .execute(
            "UPDATE users SET is_active = true, updated_at = now() WHERE id = $1",
            &[&id],
        )
        .await?;
    println!("User activated successfully");
    Ok(Ok(()))
}

/// `test_email <to_email>` (`test_email.py:21-67`).
///
/// Reads the seven `EMAIL_*` values through `get_email_configuration`
/// (`plane_mail::EmailConfiguration::load`), renders
/// `emails/test_email.html`, converts it to the plain-text alternative and
/// sends through `plane-mail`'s `EmailMultiAlternatives` bytes. Django's
/// `get_connection(timeout=30)` sets a 30 s socket timeout; the configuration
/// carries it for this command only.
///
/// # Errors
///
/// [`AdminError::CommandError`] for a missing receiver; a configuration or
/// template failure propagates as an uncaught exception (Django would print a
/// traceback and exit 1). Delivery errors are printed and are not an error.
pub async fn test_email(
    client: &Client,
    settings: &Settings,
    receiver: &str,
) -> Result<std::result::Result<(), AdminError>> {
    if receiver.is_empty() {
        return Ok(Err(AdminError::CommandError(
            "Receiver email is required".to_string(),
        )));
    }
    let configuration = EmailConfiguration::load(client, settings).await?;
    let html = render("emails/test_email.html", &serde_json::json!({}))?;
    let text = generate_plain_text_from_html(&html);
    println!("Trying to send test email...");
    let message = Message::with_html(
        "Test email from Plane",
        text,
        html,
        configuration.from.clone(),
        vec![receiver.to_string()],
    );
    // `get_connection(..., timeout=30)` (`test_email.py:37-45`).
    match plane_mail::send_with_timeout(
        &configuration,
        &message,
        Some(std::time::Duration::from_secs(30)),
    )
    .await
    {
        Ok(()) => println!("Email successfully sent"),
        Err(error) => println!("Error: Email could not be delivered due to {error}"),
    }
    Ok(Ok(()))
}

/// `getpass.getpass(prompt)` without echo suppression: the prompt goes to
/// stderr and one line is read from stdin, newline stripped.
fn prompt(text: &str) -> String {
    eprint!("{text}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    line.trim_end_matches(['\n', '\r']).to_string()
}
