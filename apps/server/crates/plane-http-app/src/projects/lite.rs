// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The nested serializers of the `projects` group, with their `*_url`
//! properties resolved.
//!
//! `ProjectLiteSerializer`, `WorkspaceLiteSerializer`, `UserLiteSerializer` and
//! `UserAdminLiteSerializer` each render a *property* that walks a nullable
//! `FileAsset` foreign key: `Project.cover_image_url`
//! (`plane/db/models/project.py:128-139`), `Workspace.logo_url`
//! (`workspace.py:145-154`) and `User.avatar_url` (`user.py:142-151`):
//!
//! ```python
//! if self.cover_image_asset:      return self.cover_image_asset.asset_url
//! if self.cover_image:            return self.cover_image
//! return None
//! ```
//!
//! Django runs one query per object because nothing `select_related`s the asset
//! (and, on the invitation/member/board routes, one or three more queries for
//! the relations themselves). A handler here loads the relations it can join in
//! the main statement, then calls [`load_asset_urls`] **once** for the union of
//! every asset id in the response and applies the result with
//! [`apply_projects`]/[`apply_workspaces`]/[`apply_users`]. That is at most two
//! round trips where Django spends four to seven.
//!
//! `FileAsset.asset_url` (`plane/db/models/asset.py:83-104`) has five branches;
//! all of them are ported, including the `None` for an entity type it does not
//! know.

use std::collections::{HashMap, HashSet};

use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_serializers::PgJson;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_user::{UserAdminLiteSerializer, UserLiteSerializer};
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use uuid::Uuid;

use crate::projects::queries;

/// `FileAsset.EntityTypeContext` values `asset_url` treats as static.
const STATIC_TYPES: &[&str] = &[
    "WORKSPACE_LOGO",
    "USER_AVATAR",
    "USER_COVER",
    "PROJECT_COVER",
];
/// Entity types whose URL embeds the workspace slug and project id.
const PROJECT_SCOPED_TYPES: &[&str] = &[
    "ISSUE_DESCRIPTION",
    "COMMENT_DESCRIPTION",
    "PAGE_DESCRIPTION",
    "DRAFT_ISSUE_DESCRIPTION",
];

/// The URL map [`load_asset_urls`] returns.
pub type AssetUrls = HashMap<Uuid, Option<String>>;

/// `FileAsset.asset_url` (`plane/db/models/asset.py:83-104`) for the columns the
/// loader selects.
///
/// A `None` slug or project/issue id renders the way Python's `f"{None}"` does
/// (`"None"`); a `None` return means the entity type has no URL, which the
/// Python expresses by falling off the end of the function.
fn asset_url(
    id: Uuid,
    entity_type: Option<&str>,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    workspace_slug: Option<&str>,
) -> Option<String> {
    let entity_type = entity_type.unwrap_or_default();
    let text = |value: Option<Uuid>| value.map_or_else(|| "None".to_string(), |id| id.to_string());
    let slug = workspace_slug.unwrap_or("None");
    if STATIC_TYPES.contains(&entity_type) {
        return Some(format!("/api/assets/v2/static/{id}/"));
    }
    if entity_type == "ISSUE_ATTACHMENT" {
        return Some(format!(
            "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{id}/",
            text(project_id),
            text(issue_id),
        ));
    }
    if PROJECT_SCOPED_TYPES.contains(&entity_type) {
        return Some(format!(
            "/api/assets/v2/workspaces/{slug}/projects/{}/{id}/",
            text(project_id),
        ));
    }
    None
}

/// `asset_url` for a set of asset ids, keyed by id, in one statement.
///
/// Ids with no row (soft-deleted or missing) are absent from the map; callers
/// treat that as Django's `RelatedObjectDoesNotExist`.
pub async fn load_asset_urls(client: &Client, ids: &HashSet<Uuid>) -> Result<AssetUrls, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<Uuid> = ids.iter().copied().collect();
    let rows = client.query(queries::ASSETS_BY_IDS.sql, &[&ids]).await?;
    let mut out = HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.get(0);
        let slug: Option<String> = row.get(4);
        out.insert(
            id,
            asset_url(id, row.get(1), row.get(2), row.get(3), slug.as_deref()),
        );
    }
    Ok(out)
}

/// The three-branch property, with the asset branch already resolved.
pub fn asset_or_text(
    asset_id: Option<Uuid>,
    text: String,
    urls: &AssetUrls,
) -> Result<Option<String>, ViewError> {
    match asset_id {
        Some(id) => match urls.get(&id) {
            Some(url) => Ok(url.clone()),
            // The FK points at a row the default manager cannot see: Django
            // raises `RelatedObjectDoesNotExist` (an `AttributeError`) out of
            // the property and the family handler answers 500.
            None => Err(ViewError::Unknown(format!(
                "the file asset {id} referenced by this row does not exist"
            ))),
        },
        None if !text.is_empty() => Ok(Some(text)),
        None => Ok(None),
    }
}

/// `UserLiteSerializer` / `UserAdminLiteSerializer` fields.
#[derive(Clone, Debug)]
pub struct LiteUser {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub avatar: String,
    pub avatar_asset_id: Option<Uuid>,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
    pub display_name: String,
    pub email: Option<String>,
    pub last_login_medium: String,
}

impl LiteUser {
    /// `UserLiteSerializer(user)`.
    pub fn lite(&self) -> UserLiteSerializer<'_> {
        UserLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
        }
    }

    /// `UserAdminLiteSerializer(user)`.
    pub fn admin_lite(&self) -> UserAdminLiteSerializer<'_> {
        UserAdminLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
            email: self.email.as_deref(),
            last_login_medium: &self.last_login_medium,
        }
    }
}

/// `WorkspaceLiteSerializer` fields.
#[derive(Clone, Debug)]
pub struct LiteWorkspace {
    pub name: String,
    pub slug: String,
    pub id: Uuid,
    pub logo: String,
    pub logo_asset_id: Option<Uuid>,
    pub logo_url: Option<String>,
}

impl LiteWorkspace {
    /// `WorkspaceLiteSerializer(workspace)`.
    pub fn lite(&self) -> WorkspaceLiteSerializer<'_> {
        WorkspaceLiteSerializer {
            name: &self.name,
            slug: &self.slug,
            id: self.id,
            logo_url: self.logo_url.as_deref(),
        }
    }
}

/// `ProjectLiteSerializer` fields.
#[derive(Clone, Debug)]
pub struct LiteProject {
    pub id: Uuid,
    pub identifier: String,
    pub name: String,
    pub cover_image: Option<String>,
    pub cover_image_asset_id: Option<Uuid>,
    pub cover_image_url: Option<String>,
    pub logo_props: JsonText,
    pub description: String,
}

impl LiteProject {
    /// `ProjectLiteSerializer(project)`.
    pub fn lite(&self) -> ProjectLiteSerializer<'_> {
        ProjectLiteSerializer {
            id: self.id,
            identifier: &self.identifier,
            name: &self.name,
            cover_image: self.cover_image.as_deref(),
            cover_image_url: self.cover_image_url.as_deref(),
            logo_props: Some(PgJson::from(&self.logo_props)),
            description: &self.description,
        }
    }
}

/// `User` rows for the nested serializers, without their `avatar_url`.
pub async fn load_users(
    client: &Client,
    ids: &HashSet<Uuid>,
) -> Result<HashMap<Uuid, LiteUser>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let wanted: Vec<Uuid> = ids.iter().copied().collect();
    let rows = client.query(queries::USERS_BY_IDS.sql, &[&wanted]).await?;
    let mut users = HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.get(0);
        users.insert(
            id,
            LiteUser {
                id,
                first_name: row.get(1),
                last_name: row.get(2),
                avatar: row.get(3),
                avatar_asset_id: row.get(4),
                avatar_url: None,
                is_bot: row.get(5),
                display_name: row.get(6),
                email: row.get(7),
                last_login_medium: row.get(8),
            },
        );
    }
    Ok(users)
}

/// `Workspace` rows for `WorkspaceLiteSerializer`, without `logo_url`.
pub async fn load_workspaces(
    client: &Client,
    ids: &HashSet<Uuid>,
) -> Result<HashMap<Uuid, LiteWorkspace>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let wanted: Vec<Uuid> = ids.iter().copied().collect();
    let rows = client
        .query(queries::WORKSPACES_BY_IDS.sql, &[&wanted])
        .await?;
    let mut workspaces = HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.get(2);
        workspaces.insert(
            id,
            LiteWorkspace {
                name: row.get(0),
                slug: row.get(1),
                id,
                logo: row.get(3),
                logo_asset_id: row.get(4),
                logo_url: None,
            },
        );
    }
    Ok(workspaces)
}

/// `Project` rows for `ProjectLiteSerializer`, without `cover_image_url`.
pub async fn load_projects(
    client: &Client,
    ids: &HashSet<Uuid>,
) -> Result<HashMap<Uuid, LiteProject>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let wanted: Vec<Uuid> = ids.iter().copied().collect();
    let rows = client
        .query(queries::PROJECTS_BY_IDS.sql, &[&wanted])
        .await?;
    let mut projects = HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.get(0);
        projects.insert(
            id,
            LiteProject {
                id,
                identifier: row.get(1),
                name: row.get(2),
                cover_image: row.get(3),
                cover_image_asset_id: row.get(4),
                cover_image_url: None,
                logo_props: row.get(5),
                description: row.get(6),
            },
        );
    }
    Ok(projects)
}

/// Fill `cover_image_url` from a resolved asset map.
pub fn apply_projects(
    projects: &mut HashMap<Uuid, LiteProject>,
    urls: &AssetUrls,
) -> Result<(), ViewError> {
    for project in projects.values_mut() {
        let text = project.cover_image.clone().unwrap_or_default();
        project.cover_image_url = asset_or_text(project.cover_image_asset_id, text, urls)?;
    }
    Ok(())
}

/// Fill `logo_url` from a resolved asset map.
pub fn apply_workspaces(
    workspaces: &mut HashMap<Uuid, LiteWorkspace>,
    urls: &AssetUrls,
) -> Result<(), ViewError> {
    for workspace in workspaces.values_mut() {
        let text = workspace.logo.clone();
        workspace.logo_url = asset_or_text(workspace.logo_asset_id, text, urls)?;
    }
    Ok(())
}

/// Fill `avatar_url` from a resolved asset map.
pub fn apply_users(users: &mut HashMap<Uuid, LiteUser>, urls: &AssetUrls) -> Result<(), ViewError> {
    for user in users.values_mut() {
        let text = user.avatar.clone();
        user.avatar_url = asset_or_text(user.avatar_asset_id, text, urls)?;
    }
    Ok(())
}

/// The union of the asset ids of the three maps, for one [`load_asset_urls`].
pub fn asset_ids(
    projects: &HashMap<Uuid, LiteProject>,
    workspaces: &HashMap<Uuid, LiteWorkspace>,
    users: &HashMap<Uuid, LiteUser>,
) -> HashSet<Uuid> {
    projects
        .values()
        .filter_map(|project| project.cover_image_asset_id)
        .chain(
            workspaces
                .values()
                .filter_map(|workspace| workspace.logo_asset_id),
        )
        .chain(users.values().filter_map(|user| user.avatar_asset_id))
        .collect()
}
