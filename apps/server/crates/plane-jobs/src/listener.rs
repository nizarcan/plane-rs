// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `LISTEN plane_jobs_jobs`: the wake-up half of D8.
//!
//! A long-lived raw `tokio-postgres` connection (not a pool member, because
//! notifications arrive on the connection stream and a pool would recycle it)
//! listens for `enqueue`'s `pg_notify`. The worker also polls, so a missed
//! notification or a listener reconnect delays a job rather than losing it.

use std::time::Duration;

use plane_core::config::Settings;
use plane_db::pool::PostgresSettings;
use plane_db::url::DatabaseConfigs;
use tokio::sync::mpsc;
use tokio_postgres::AsyncMessage;

use crate::queue::QueueError;

/// A wake-up receiver. Dropping it stops the listener on its next send.
pub struct Listener {
    rx: mpsc::Receiver<()>,
}

impl Listener {
    /// Start the listener loop. The first connection is attempted immediately;
    /// a database failure is retried in the background, because the worker's
    /// poll fallback must keep the process alive while Postgres restarts.
    ///
    /// # Errors
    ///
    /// Returns a configuration error immediately (a malformed `DATABASE_URL`);
    /// connection errors are logged and retried.
    pub fn connect(settings: &Settings) -> Result<Self, QueueError> {
        // Parse the configuration now so a bad URL is a startup error, not a
        // permanently silent listener.
        let configs = DatabaseConfigs::from_settings(&settings.database)?;
        PostgresSettings::build(&configs.primary)?;

        let (tx, rx) = mpsc::channel(1);
        let settings = settings.clone();
        tokio::spawn(async move {
            loop {
                match listen_once(&settings, &tx).await {
                    Ok(()) => {}
                    Err(error) => {
                        tracing::warn!(
                            target: "plane.jobs",
                            %error,
                            "job listener disconnected; retrying"
                        );
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        Ok(Self { rx })
    }

    /// Wait for the next notification. `None` means the listener stopped.
    pub async fn recv(&mut self) -> Option<()> {
        self.rx.recv().await
    }
}

async fn listen_once(settings: &Settings, tx: &mpsc::Sender<()>) -> Result<(), QueueError> {
    plane_db::tls::install_crypto_provider();
    let configs = DatabaseConfigs::from_settings(&settings.database)?;
    let postgres = PostgresSettings::build(&configs.primary)?;
    let (client, mut connection) = postgres.config.connect(postgres.connector).await?;
    client
        .batch_execute(&format!("LISTEN {}", crate::queue::NOTIFY_CHANNEL))
        .await?;
    tracing::info!(
        target: "plane.jobs",
        channel = crate::queue::NOTIFY_CHANNEL,
        "job listener connected"
    );
    while let Some(message) = std::future::poll_fn(|cx| connection.poll_message(cx)).await {
        let message = message?;
        if matches!(message, AsyncMessage::Notification(_)) && tx.send(()).await.is_err() {
            return Ok(());
        }
    }
    Ok(())
}
