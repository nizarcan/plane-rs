// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Unit and golden tests for the Celery publisher.

mod export_args;
mod golden;
mod message;
mod repr;
mod url;

#[cfg(feature = "celery")]
mod live;
