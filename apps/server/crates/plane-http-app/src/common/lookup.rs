// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two lookups every workspace-scoped view starts with.
//!
//! ```ignore
//! let workspace = lookup::workspace_by_slug(client, slug).await?;
//! let project = lookup::project_in_workspace(client, workspace.id, project_id)?;
//! ```
//!
//! Both reproduce `QuerySet.get(**kwargs)` as Django runs it:
//!
//! * the default manager's predicate — `deleted_at IS NULL`
//!   (`plane/db/mixins.py:57`) — is part of the statement;
//! * a miss raises `Model.DoesNotExist`, a subclass of
//!   `ObjectDoesNotExist`, which Plane's `handle_exception` renders as
//!   `404 {"error": "The required object does not exist."}`
//!   (`plane/app/views/base.py:202-208`, HTTP_CONTRACT §11.2). Hence
//!   [`Error::NotFound`] → [`ViewError::ObjectDoesNotExist`];
//! * an unparsable UUID kwarg raises `ValidationError`, rendered as
//!   `400 {"error": "Please provide valid detail"}` (`base.py:216-220`).
//!
//! A view that catches `DoesNotExist` itself (some return a 400 with a
//! different body) matches on [`Error::NotFound`] and renders its own response.
//! Do not add a third behaviour here.

use deadpool_postgres::Client;
use plane_db::models::project::Project;
use plane_db::models::workspace::Workspace;
use plane_drf::error::ViewError;
use uuid::Uuid;

use crate::common::queries;

/// Why a lookup did not produce a row.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `Model.DoesNotExist` (`get()` found nothing).
    #[error("the object does not exist")]
    NotFound,
    /// `ValidationError` from `UUIDField.to_python`.
    #[error("invalid UUID {value:?} for {field}")]
    InvalidUuid { field: &'static str, value: String },
    /// The database failed.
    #[error(transparent)]
    Db(#[from] plane_db::DbError),
}

impl From<tokio_postgres::Error> for Error {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Db(plane_db::DbError::Postgres(error))
    }
}

impl From<Error> for ViewError {
    fn from(error: Error) -> Self {
        match error {
            // `plane/app/views/base.py:202-208`.
            Error::NotFound => ViewError::ObjectDoesNotExist,
            // `plane/app/views/base.py:216-220`.
            Error::InvalidUuid { .. } => ViewError::Validation,
            Error::Db(error) => ViewError::from(error),
        }
    }
}

/// `Workspace.objects.get(slug=slug)`.
pub async fn workspace_by_slug(client: &Client, slug: &str) -> Result<Workspace, Error> {
    let statement = client
        .prepare_cached(queries::WORKSPACE_BY_SLUG.sql)
        .await?;
    match client.query_opt(&statement, &[&slug]).await? {
        Some(row) => Ok(Workspace::from_row(&row, 0)?),
        None => Err(Error::NotFound),
    }
}

/// `Project.objects.get(pk=project_id, workspace__slug=slug)` with the
/// workspace already loaded.
pub async fn project_in_workspace(
    client: &Client,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<Project, Error> {
    let statement = client
        .prepare_cached(queries::PROJECT_IN_WORKSPACE.sql)
        .await?;
    match client
        .query_opt(&statement, &[&workspace_id, &project_id])
        .await?
    {
        Some(row) => Ok(Project::from_row(&row, 0)?),
        None => Err(Error::NotFound),
    }
}

/// A UUID URL kwarg, parsed the way Django's `UUIDField.to_python` accepts it.
///
/// `Uuid::parse_str` takes the same three forms Django's `uuid.UUID(hex=…)`
/// does: hyphenated, 32 hex digits, `urn:uuid:` and the braced form.
pub fn uuid_kwarg(field: &'static str, value: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(value).map_err(|_| Error::InvalidUuid {
        field,
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_kwargs_accept_what_django_accepts() {
        let canonical = "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f";
        assert_eq!(
            uuid_kwarg("project_id", canonical).expect("hyphenated"),
            Uuid::parse_str(canonical).expect("uuid")
        );
        assert!(uuid_kwarg("project_id", "0f0e7a3a2f5c4b1e9c3f1a2b3c4d5e6f").is_ok());
        assert!(matches!(
            uuid_kwarg("project_id", "not-a-uuid"),
            Err(Error::InvalidUuid {
                field: "project_id",
                ..
            })
        ));
    }

    #[test]
    fn errors_map_to_the_family_handle_exception_bodies() {
        assert_eq!(
            ViewError::from(Error::NotFound),
            ViewError::ObjectDoesNotExist
        );
        assert_eq!(
            ViewError::from(Error::InvalidUuid {
                field: "project_id",
                value: "x".into()
            }),
            ViewError::Validation
        );
    }
}
