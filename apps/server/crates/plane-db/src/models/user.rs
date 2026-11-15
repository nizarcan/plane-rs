// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/user.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `Account` — table `accounts`, generated from `apps/api/plane/db/models/user.py`.
///
/// 13 columns, not soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    /// `created_at` — `DateTimeField`, `auto_now_add`.
    pub created_at: DateTime<Utc>,
    /// `updated_at` — `DateTimeField`, `auto_now`.
    pub updated_at: DateTime<Utc>,
    /// `id` — `UUIDField`, `max_length=32`, `primary key`, `unique`, `db_index`. Default: `uuid.uuid4`.
    pub id: Uuid,
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `provider_account_id` — `CharField`, `max_length=255`.
    pub provider_account_id: String,
    /// `provider` — `CharField`.
    pub provider: String,
    /// `access_token` — `TextField`.
    pub access_token: String,
    /// `access_token_expired_at` — `DateTimeField`, nullable.
    pub access_token_expired_at: Option<DateTime<Utc>>,
    /// `refresh_token` — `TextField`, nullable.
    pub refresh_token: Option<String>,
    /// `refresh_token_expired_at` — `DateTimeField`, nullable.
    pub refresh_token_expired_at: Option<DateTime<Utc>>,
    /// `last_connected_at` — `DateTimeField`. Default: `django.utils.timezone.now`.
    pub last_connected_at: DateTime<Utc>,
    /// `id_token` — `TextField`.
    pub id_token: String,
    /// `metadata` — `JSONField`. Default: `builtins.dict`.
    pub metadata: JsonText,
}

impl Account {
    /// The table this row is read from (`accounts`).
    pub const TABLE: &'static str = "accounts";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"created_at\"",
        ", {alias}.\"updated_at\"",
        ", {alias}.\"id\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"provider_account_id\"",
        ", {alias}.\"provider\"",
        ", {alias}.\"access_token\"",
        ", {alias}.\"access_token_expired_at\"",
        ", {alias}.\"refresh_token\"",
        ", {alias}.\"refresh_token_expired_at\"",
        ", {alias}.\"last_connected_at\"",
        ", {alias}.\"id_token\"",
        ", {alias}.\"metadata\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (13).
    pub const COLUMN_COUNT: usize = 13;

    /// Default ordering (`Meta.ordering = ['-created_at']`).
    ///
    /// Django's SQL: `"created_at" DESC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"created_at\" DESC";

    /// Manager `objects` — `django.db.models.manager.Manager.get_queryset`
    /// (`site-packages/django/db/models/manager.py:150`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         """
    ///         Return a new QuerySet object. Subclasses can override this method to
    ///         customize the behavior of the Manager.
    ///         """
    ///         return self._queryset_class(model=self.model, using=self._db, hints=self._hints)
    /// ```
    ///
    /// Django WHERE: none (every row).
    pub const OBJECTS: &'static str = "TRUE";

    /// Decode this model's row, starting at column `start`.
    ///
    /// Columns are read by index, which is why the select list and this
    /// function are generated together.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {
        Ok(Self {
            created_at: row.try_get(start)?,
            updated_at: row.try_get(start + 1)?,
            id: row.try_get(start + 2)?,
            user_id: row.try_get(start + 3)?,
            provider_account_id: row.try_get(start + 4)?,
            provider: row.try_get(start + 5)?,
            access_token: row.try_get(start + 6)?,
            access_token_expired_at: row.try_get(start + 7)?,
            refresh_token: row.try_get(start + 8)?,
            refresh_token_expired_at: row.try_get(start + 9)?,
            last_connected_at: row.try_get(start + 10)?,
            id_token: row.try_get(start + 11)?,
            metadata: row.try_get(start + 12)?,
        })
    }
}

/// `Profile` — table `profiles`, generated from `apps/api/plane/db/models/user.py`.
///
/// 29 columns, not soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// `created_at` — `DateTimeField`, `auto_now_add`.
    pub created_at: DateTime<Utc>,
    /// `updated_at` — `DateTimeField`, `auto_now`.
    pub updated_at: DateTime<Utc>,
    /// `id` — `UUIDField`, `max_length=32`, `primary key`, `unique`, `db_index`. Default: `uuid.uuid4`.
    pub id: Uuid,
    /// `user` → `db.User` (`users.id`, `CASCADE`, one-to-one).
    pub user_id: Uuid,
    /// `theme` — `JSONField`. Default: `builtins.dict`.
    pub theme: JsonText,
    /// `is_app_rail_docked` — `BooleanField`. Default: `True`.
    pub is_app_rail_docked: bool,
    /// `is_tour_completed` — `BooleanField`. Default: `False`.
    pub is_tour_completed: bool,
    /// `onboarding_step` — `JSONField`. Default: `plane.db.models.user.get_default_onboarding`.
    pub onboarding_step: JsonText,
    /// `use_case` — `TextField`, nullable.
    pub use_case: Option<String>,
    /// `role` — `CharField`, `max_length=300`, nullable.
    pub role: Option<String>,
    /// `is_onboarded` — `BooleanField`. Default: `False`.
    pub is_onboarded: bool,
    /// `last_workspace_id` — `UUIDField`, `max_length=32`, nullable.
    pub last_workspace_id: Option<Uuid>,
    /// `billing_address_country` — `CharField`, `max_length=255`. Default: `'INDIA'`.
    pub billing_address_country: String,
    /// `billing_address` — `JSONField`, nullable.
    pub billing_address: Option<JsonText>,
    /// `has_billing_address` — `BooleanField`. Default: `False`.
    pub has_billing_address: bool,
    /// `company_name` — `CharField`, `max_length=255`.
    pub company_name: String,
    /// `notification_view_mode` — `CharField`, `max_length=255`. Default: `Profile.NotificationViewMode.FULL`.
    pub notification_view_mode: String,
    /// `is_smooth_cursor_enabled` — `BooleanField`. Default: `False`.
    pub is_smooth_cursor_enabled: bool,
    /// `is_mobile_onboarded` — `BooleanField`. Default: `False`.
    pub is_mobile_onboarded: bool,
    /// `mobile_onboarding_step` — `JSONField`. Default: `plane.db.models.user.get_mobile_default_onboarding`.
    pub mobile_onboarding_step: JsonText,
    /// `mobile_timezone_auto_set` — `BooleanField`. Default: `False`.
    pub mobile_timezone_auto_set: bool,
    /// `language` — `CharField`, `max_length=255`. Default: `'en'`.
    pub language: String,
    /// `start_of_the_week` — `PositiveSmallIntegerField`. Default: `0`.
    pub start_of_the_week: i16,
    /// `goals` — `JSONField`. Default: `builtins.dict`.
    pub goals: JsonText,
    /// `background_color` — `CharField`, `max_length=255`. Default: `plane.utils.color.get_random_color`.
    pub background_color: String,
    /// `is_navigation_tour_completed` — `BooleanField`. Default: `False`.
    pub is_navigation_tour_completed: bool,
    /// `has_marketing_email_consent` — `BooleanField`. Default: `False`.
    pub has_marketing_email_consent: bool,
    /// `is_subscribed_to_changelog` — `BooleanField`. Default: `False`.
    pub is_subscribed_to_changelog: bool,
    /// `product_tour` — `JSONField`. Default: `plane.db.models.user.get_default_product_tour`.
    pub product_tour: JsonText,
}

impl Profile {
    /// The table this row is read from (`profiles`).
    pub const TABLE: &'static str = "profiles";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"created_at\"",
        ", {alias}.\"updated_at\"",
        ", {alias}.\"id\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"theme\"",
        ", {alias}.\"is_app_rail_docked\"",
        ", {alias}.\"is_tour_completed\"",
        ", {alias}.\"onboarding_step\"",
        ", {alias}.\"use_case\"",
        ", {alias}.\"role\"",
        ", {alias}.\"is_onboarded\"",
        ", {alias}.\"last_workspace_id\"",
        ", {alias}.\"billing_address_country\"",
        ", {alias}.\"billing_address\"",
        ", {alias}.\"has_billing_address\"",
        ", {alias}.\"company_name\"",
        ", {alias}.\"notification_view_mode\"",
        ", {alias}.\"is_smooth_cursor_enabled\"",
        ", {alias}.\"is_mobile_onboarded\"",
        ", {alias}.\"mobile_onboarding_step\"",
        ", {alias}.\"mobile_timezone_auto_set\"",
        ", {alias}.\"language\"",
        ", {alias}.\"start_of_the_week\"",
        ", {alias}.\"goals\"",
        ", {alias}.\"background_color\"",
        ", {alias}.\"is_navigation_tour_completed\"",
        ", {alias}.\"has_marketing_email_consent\"",
        ", {alias}.\"is_subscribed_to_changelog\"",
        ", {alias}.\"product_tour\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (29).
    pub const COLUMN_COUNT: usize = 29;

    /// Default ordering (`Meta.ordering = ['-created_at']`).
    ///
    /// Django's SQL: `"created_at" DESC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"created_at\" DESC";

    /// Manager `objects` — `django.db.models.manager.Manager.get_queryset`
    /// (`site-packages/django/db/models/manager.py:150`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         """
    ///         Return a new QuerySet object. Subclasses can override this method to
    ///         customize the behavior of the Manager.
    ///         """
    ///         return self._queryset_class(model=self.model, using=self._db, hints=self._hints)
    /// ```
    ///
    /// Django WHERE: none (every row).
    pub const OBJECTS: &'static str = "TRUE";

    /// Decode this model's row, starting at column `start`.
    ///
    /// Columns are read by index, which is why the select list and this
    /// function are generated together.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {
        Ok(Self {
            created_at: row.try_get(start)?,
            updated_at: row.try_get(start + 1)?,
            id: row.try_get(start + 2)?,
            user_id: row.try_get(start + 3)?,
            theme: row.try_get(start + 4)?,
            is_app_rail_docked: row.try_get(start + 5)?,
            is_tour_completed: row.try_get(start + 6)?,
            onboarding_step: row.try_get(start + 7)?,
            use_case: row.try_get(start + 8)?,
            role: row.try_get(start + 9)?,
            is_onboarded: row.try_get(start + 10)?,
            last_workspace_id: row.try_get(start + 11)?,
            billing_address_country: row.try_get(start + 12)?,
            billing_address: row.try_get(start + 13)?,
            has_billing_address: row.try_get(start + 14)?,
            company_name: row.try_get(start + 15)?,
            notification_view_mode: row.try_get(start + 16)?,
            is_smooth_cursor_enabled: row.try_get(start + 17)?,
            is_mobile_onboarded: row.try_get(start + 18)?,
            mobile_onboarding_step: row.try_get(start + 19)?,
            mobile_timezone_auto_set: row.try_get(start + 20)?,
            language: row.try_get(start + 21)?,
            start_of_the_week: row.try_get(start + 22)?,
            goals: row.try_get(start + 23)?,
            background_color: row.try_get(start + 24)?,
            is_navigation_tour_completed: row.try_get(start + 25)?,
            has_marketing_email_consent: row.try_get(start + 26)?,
            is_subscribed_to_changelog: row.try_get(start + 27)?,
            product_tour: row.try_get(start + 28)?,
        })
    }
}

/// `User` — table `users`, generated from `apps/api/plane/db/models/user.py`.
///
/// 40 columns, not soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct User {
    /// `password` — `CharField`, `max_length=128`.
    pub password: String,
    /// `last_login` — `DateTimeField`, nullable.
    pub last_login: Option<DateTime<Utc>>,
    /// `id` — `UUIDField`, `max_length=32`, `primary key`, `unique`, `db_index`. Default: `uuid.uuid4`.
    pub id: Uuid,
    /// `username` — `CharField`, `max_length=128`, `unique`.
    pub username: String,
    /// `mobile_number` — `CharField`, `max_length=255`, nullable.
    pub mobile_number: Option<String>,
    /// `email` — `CharField`, `max_length=255`, `unique`, nullable.
    pub email: Option<String>,
    /// `display_name` — `CharField`, `max_length=255`. Default: `''`.
    pub display_name: String,
    /// `first_name` — `CharField`, `max_length=255`.
    pub first_name: String,
    /// `last_name` — `CharField`, `max_length=255`.
    pub last_name: String,
    /// `avatar` — `TextField`.
    pub avatar: String,
    /// `avatar_asset` → `db.FileAsset` (`file_assets.id`, `SET_NULL`), nullable.
    pub avatar_asset_id: Option<Uuid>,
    /// `cover_image` — `URLField`, `max_length=800`, nullable.
    pub cover_image: Option<String>,
    /// `cover_image_asset` → `db.FileAsset` (`file_assets.id`, `SET_NULL`), nullable.
    pub cover_image_asset_id: Option<Uuid>,
    /// `date_joined` — `DateTimeField`, `auto_now_add`.
    pub date_joined: DateTime<Utc>,
    /// `created_at` — `DateTimeField`, `auto_now_add`.
    pub created_at: DateTime<Utc>,
    /// `updated_at` — `DateTimeField`, `auto_now`.
    pub updated_at: DateTime<Utc>,
    /// `last_location` — `CharField`, `max_length=255`.
    pub last_location: String,
    /// `created_location` — `CharField`, `max_length=255`.
    pub created_location: String,
    /// `is_superuser` — `BooleanField`. Default: `False`.
    pub is_superuser: bool,
    /// `is_managed` — `BooleanField`. Default: `False`.
    pub is_managed: bool,
    /// `is_password_expired` — `BooleanField`. Default: `False`.
    pub is_password_expired: bool,
    /// `is_active` — `BooleanField`. Default: `True`.
    pub is_active: bool,
    /// `is_staff` — `BooleanField`. Default: `False`.
    pub is_staff: bool,
    /// `is_email_verified` — `BooleanField`. Default: `False`.
    pub is_email_verified: bool,
    /// `is_password_autoset` — `BooleanField`. Default: `False`.
    pub is_password_autoset: bool,
    /// `is_password_reset_required` — `BooleanField`. Default: `False`.
    pub is_password_reset_required: bool,
    /// `token` — `CharField`, `max_length=64`.
    pub token: String,
    /// `last_active` — `DateTimeField`, nullable. Default: `django.utils.timezone.now`.
    pub last_active: Option<DateTime<Utc>>,
    /// `last_login_time` — `DateTimeField`, nullable.
    pub last_login_time: Option<DateTime<Utc>>,
    /// `last_logout_time` — `DateTimeField`, nullable.
    pub last_logout_time: Option<DateTime<Utc>>,
    /// `last_login_ip` — `CharField`, `max_length=255`.
    pub last_login_ip: String,
    /// `last_logout_ip` — `CharField`, `max_length=255`.
    pub last_logout_ip: String,
    /// `last_login_medium` — `CharField`, `max_length=20`. Default: `'email'`.
    pub last_login_medium: String,
    /// `last_login_uagent` — `TextField`.
    pub last_login_uagent: String,
    /// `token_updated_at` — `DateTimeField`, nullable.
    pub token_updated_at: Option<DateTime<Utc>>,
    /// `is_bot` — `BooleanField`. Default: `False`.
    pub is_bot: bool,
    /// `bot_type` — `CharField`, `max_length=30`, nullable.
    pub bot_type: Option<String>,
    /// `user_timezone` — `CharField`, `max_length=255`. Default: `'UTC'`.
    pub user_timezone: String,
    /// `is_email_valid` — `BooleanField`. Default: `False`.
    pub is_email_valid: bool,
    /// `masked_at` — `DateTimeField`, nullable.
    pub masked_at: Option<DateTime<Utc>>,
}

impl User {
    /// The table this row is read from (`users`).
    pub const TABLE: &'static str = "users";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"password\"",
        ", {alias}.\"last_login\"",
        ", {alias}.\"id\"",
        ", {alias}.\"username\"",
        ", {alias}.\"mobile_number\"",
        ", {alias}.\"email\"",
        ", {alias}.\"display_name\"",
        ", {alias}.\"first_name\"",
        ", {alias}.\"last_name\"",
        ", {alias}.\"avatar\"",
        ", {alias}.\"avatar_asset_id\"",
        ", {alias}.\"cover_image\"",
        ", {alias}.\"cover_image_asset_id\"",
        ", {alias}.\"date_joined\"",
        ", {alias}.\"created_at\"",
        ", {alias}.\"updated_at\"",
        ", {alias}.\"last_location\"",
        ", {alias}.\"created_location\"",
        ", {alias}.\"is_superuser\"",
        ", {alias}.\"is_managed\"",
        ", {alias}.\"is_password_expired\"",
        ", {alias}.\"is_active\"",
        ", {alias}.\"is_staff\"",
        ", {alias}.\"is_email_verified\"",
        ", {alias}.\"is_password_autoset\"",
        ", {alias}.\"is_password_reset_required\"",
        ", {alias}.\"token\"",
        ", {alias}.\"last_active\"",
        ", {alias}.\"last_login_time\"",
        ", {alias}.\"last_logout_time\"",
        ", {alias}.\"last_login_ip\"",
        ", {alias}.\"last_logout_ip\"",
        ", {alias}.\"last_login_medium\"",
        ", {alias}.\"last_login_uagent\"",
        ", {alias}.\"token_updated_at\"",
        ", {alias}.\"is_bot\"",
        ", {alias}.\"bot_type\"",
        ", {alias}.\"user_timezone\"",
        ", {alias}.\"is_email_valid\"",
        ", {alias}.\"masked_at\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (40).
    pub const COLUMN_COUNT: usize = 40;

    /// Default ordering (`Meta.ordering = ['-created_at']`).
    ///
    /// Django's SQL: `"created_at" DESC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"created_at\" DESC";

    /// Manager `objects` — `django.contrib.auth.models.UserManager.get_queryset`
    /// (`site-packages/django/db/models/manager.py:150`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         """
    ///         Return a new QuerySet object. Subclasses can override this method to
    ///         customize the behavior of the Manager.
    ///         """
    ///         return self._queryset_class(model=self.model, using=self._db, hints=self._hints)
    /// ```
    ///
    /// Django WHERE: none (every row).
    pub const OBJECTS: &'static str = "TRUE";

    /// Decode this model's row, starting at column `start`.
    ///
    /// Columns are read by index, which is why the select list and this
    /// function are generated together.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {
        Ok(Self {
            password: row.try_get(start)?,
            last_login: row.try_get(start + 1)?,
            id: row.try_get(start + 2)?,
            username: row.try_get(start + 3)?,
            mobile_number: row.try_get(start + 4)?,
            email: row.try_get(start + 5)?,
            display_name: row.try_get(start + 6)?,
            first_name: row.try_get(start + 7)?,
            last_name: row.try_get(start + 8)?,
            avatar: row.try_get(start + 9)?,
            avatar_asset_id: row.try_get(start + 10)?,
            cover_image: row.try_get(start + 11)?,
            cover_image_asset_id: row.try_get(start + 12)?,
            date_joined: row.try_get(start + 13)?,
            created_at: row.try_get(start + 14)?,
            updated_at: row.try_get(start + 15)?,
            last_location: row.try_get(start + 16)?,
            created_location: row.try_get(start + 17)?,
            is_superuser: row.try_get(start + 18)?,
            is_managed: row.try_get(start + 19)?,
            is_password_expired: row.try_get(start + 20)?,
            is_active: row.try_get(start + 21)?,
            is_staff: row.try_get(start + 22)?,
            is_email_verified: row.try_get(start + 23)?,
            is_password_autoset: row.try_get(start + 24)?,
            is_password_reset_required: row.try_get(start + 25)?,
            token: row.try_get(start + 26)?,
            last_active: row.try_get(start + 27)?,
            last_login_time: row.try_get(start + 28)?,
            last_logout_time: row.try_get(start + 29)?,
            last_login_ip: row.try_get(start + 30)?,
            last_logout_ip: row.try_get(start + 31)?,
            last_login_medium: row.try_get(start + 32)?,
            last_login_uagent: row.try_get(start + 33)?,
            token_updated_at: row.try_get(start + 34)?,
            is_bot: row.try_get(start + 35)?,
            bot_type: row.try_get(start + 36)?,
            user_timezone: row.try_get(start + 37)?,
            is_email_valid: row.try_get(start + 38)?,
            masked_at: row.try_get(start + 39)?,
        })
    }
}

/// `User_groups` — table `users_groups`, generated from `apps/api/plane/db/models/user.py`.
///
/// 3 columns, not soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct UserGroups {
    /// `id` — `BigAutoField`, `primary key`, `unique`.
    pub id: i64,
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `group` → `auth.Group` (`auth_group.id`, `CASCADE`).
    pub group_id: i32,
}

impl UserGroups {
    /// The table this row is read from (`users_groups`).
    pub const TABLE: &'static str = "users_groups";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"id\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"group_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (3).
    pub const COLUMN_COUNT: usize = 3;

    /// No `Meta.ordering`: Django leaves the row order to Postgres.
    pub const ORDER_BY: &'static str = "";

    /// Manager `objects` — `django.db.models.manager.Manager.get_queryset`
    /// (`site-packages/django/db/models/manager.py:150`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         """
    ///         Return a new QuerySet object. Subclasses can override this method to
    ///         customize the behavior of the Manager.
    ///         """
    ///         return self._queryset_class(model=self.model, using=self._db, hints=self._hints)
    /// ```
    ///
    /// Django WHERE: none (every row).
    pub const OBJECTS: &'static str = "TRUE";

    /// Decode this model's row, starting at column `start`.
    ///
    /// Columns are read by index, which is why the select list and this
    /// function are generated together.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get(start)?,
            user_id: row.try_get(start + 1)?,
            group_id: row.try_get(start + 2)?,
        })
    }
}

/// `User_user_permissions` — table `users_user_permissions`, generated from `apps/api/plane/db/models/user.py`.
///
/// 3 columns, not soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct UserUserPermissions {
    /// `id` — `BigAutoField`, `primary key`, `unique`.
    pub id: i64,
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `permission` → `auth.Permission` (`auth_permission.id`, `CASCADE`).
    pub permission_id: i32,
}

impl UserUserPermissions {
    /// The table this row is read from (`users_user_permissions`).
    pub const TABLE: &'static str = "users_user_permissions";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"id\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"permission_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (3).
    pub const COLUMN_COUNT: usize = 3;

    /// No `Meta.ordering`: Django leaves the row order to Postgres.
    pub const ORDER_BY: &'static str = "";

    /// Manager `objects` — `django.db.models.manager.Manager.get_queryset`
    /// (`site-packages/django/db/models/manager.py:150`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         """
    ///         Return a new QuerySet object. Subclasses can override this method to
    ///         customize the behavior of the Manager.
    ///         """
    ///         return self._queryset_class(model=self.model, using=self._db, hints=self._hints)
    /// ```
    ///
    /// Django WHERE: none (every row).
    pub const OBJECTS: &'static str = "TRUE";

    /// Decode this model's row, starting at column `start`.
    ///
    /// Columns are read by index, which is why the select list and this
    /// function are generated together.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get(start)?,
            user_id: row.try_get(start + 1)?,
            permission_id: row.try_get(start + 2)?,
        })
    }
}
