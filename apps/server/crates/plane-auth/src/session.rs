// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's database session backend (`django/contrib/sessions/backends/db.py`,
//! `django/contrib/sessions/backends/base.py`) as subclassed by
//! `plane/db/models/session.py`, plus the response decision from
//! `plane/authentication/middleware/session.py`.
//!
//! Every method cites the Python line it ports. The state machine is
//! deliberately the same shape as `SessionBase`:
//!
//! * `key` is `_session_key`, validated by `_validate_session_key`
//!   (`base.py:178-198`): truthy and at least 8 characters, otherwise `None`;
//! * `cache` is `_session_cache`; `None` means "not loaded yet", so that
//!   touching the session is the only thing that reads the `sessions` row;
//! * `accessed` and `modified` are the flags the middleware reads.
//!
//! `Session` model (`plane/db/models/session.py:17-56`):
//!
//! | column | source |
//! | --- | --- |
//! | `session_key` | `get_random_string(128, ascii_lowercase + digits)`, retried while `exists()` |
//! | `session_data` | `encode()`: signed, optionally zlib-compressed JSON |
//! | `expire_date` | `get_expiry_date()` at save time |
//! | `user_id` | `session_data["_auth_user_id"]` (a `CharField`, not an FK) |
//! | `device_info` | `session_data["device_info"]` when it is a dict, else `NULL` |

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, TimeDelta, Utc};
use deadpool_postgres::Client;
use plane_core::config::Settings;
use uuid::Uuid;

use crate::cookies::http_date;
use crate::error::AuthError;
use crate::json::Json;
use crate::queries;
use crate::random::{VALID_KEY_CHARS, get_random_string};
use crate::signing::{self, SESSION_KEY_SALT, SigningError};

/// `SESSION_KEY`.
pub const SESSION_KEY: &str = "_auth_user_id";
/// `BACKEND_SESSION_KEY`.
pub const BACKEND_SESSION_KEY: &str = "_auth_user_backend";
/// `HASH_SESSION_KEY`.
pub const HASH_SESSION_KEY: &str = "_auth_user_hash";
/// `_session_expiry`, the payload key `set_expiry` writes.
pub const EXPIRY_SESSION_KEY: &str = "_session_expiry";
/// `plane/settings/common.py` does not set `SESSION_EXPIRE_AT_BROWSER_CLOSE`, so
/// Django's global default applies (`django/conf/global_settings.py`).
pub const SESSION_EXPIRE_AT_BROWSER_CLOSE: bool = false;

/// Why a save failed.
///
/// The distinction is Django's (`db.py:117-140`): an `IntegrityError` on a
/// `must_create` insert is `CreateError` and the key is retried; a
/// `DatabaseError` on an update (including "forced update did not affect any
/// rows") is `UpdateError`, which the middleware turns into
/// `SessionInterrupted` — an HTTP 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The generated key already exists; `create()` tries again.
    Create,
    /// The row vanished between load and save.
    Update,
    /// Anything else.
    Database(AuthError),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create => write!(f, "session key collision"),
            Self::Update => write!(
                f,
                "the request's session was deleted before the request completed"
            ),
            Self::Database(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<AuthError> for SaveError {
    fn from(error: AuthError) -> Self {
        Self::Database(error)
    }
}

impl From<tokio_postgres::Error> for SaveError {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Database(AuthError::from(error))
    }
}

/// `SessionBase` plus the Plane `Session` row mapping.
#[derive(Debug, Clone)]
pub struct SessionStore {
    key: Option<String>,
    cache: Option<Json>,
    /// `request.session.accessed`.
    pub accessed: bool,
    /// `request.session.modified`.
    pub modified: bool,
}

impl SessionStore {
    /// `SessionStore(session_key)`: `_set_session_key` validates the key
    /// (`base.py:178-198`).
    pub fn new(session_key: Option<String>) -> Self {
        Self {
            key: session_key.filter(|key| !key.is_empty() && key.chars().count() >= 8),
            cache: None,
            accessed: false,
            modified: false,
        }
    }

    /// `key_salt` (`base.py:66-69`).
    pub const KEY_SALT: &'static str = SESSION_KEY_SALT;

    pub fn session_key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// The payload, when it has been loaded.
    pub fn loaded(&self) -> Option<&Json> {
        self.cache.as_ref()
    }

    /// `is_empty()` (`base.py:185-190`): no key *and* no cached data.
    pub fn is_empty(&self) -> bool {
        if self.key.is_some() {
            return false;
        }
        match &self.cache {
            None => true,
            Some(cache) => cache
                .as_object()
                .map(|entries| entries.is_empty())
                .unwrap_or(true),
        }
    }

    /// `session.get(key)`. Only the cache is consulted; callers that need the
    /// database use [`Self::get_loaded`].
    pub fn get_cached(&self, key: &str) -> Option<&Json> {
        self.cache.as_ref().and_then(|cache| cache.get(key))
    }

    /// `_get_session(no_load=False)`: mark accessed and load once.
    pub async fn load(&mut self, client: &Client, secret_key: &str) -> Result<(), AuthError> {
        self.accessed = true;
        if self.cache.is_some() {
            return Ok(());
        }
        let Some(key) = self.key.clone() else {
            self.cache = Some(Json::empty_object());
            return Ok(());
        };
        let statement = client
            .prepare_cached(queries::LOAD_SESSION_BY_KEY.sql)
            .await?;
        let row = client.query_opt(&statement, &[&key]).await?;
        match row {
            // `_get_session_from_db`: a miss sets `_session_key = None`.
            None => {
                self.key = None;
                self.cache = Some(Json::empty_object());
            }
            Some(row) => {
                let session_data: String = row.get(0);
                self.cache = Some(self.decode(&session_data, secret_key));
            }
        }
        Ok(())
    }

    /// `session[key]`, loading the payload first (`_session` via
    /// `_get_session`).
    pub async fn get(
        &mut self,
        client: &Client,
        secret_key: &str,
        key: &str,
    ) -> Result<Option<Json>, AuthError> {
        self.load(client, secret_key).await?;
        Ok(self.get_cached(key).cloned())
    }

    /// `key in session`.
    pub async fn contains(
        &mut self,
        client: &Client,
        secret_key: &str,
        key: &str,
    ) -> Result<bool, AuthError> {
        Ok(self.get(client, secret_key, key).await?.is_some())
    }

    /// `session[key] = value` (`base.py:57-60`): loads, stores, marks modified.
    pub async fn set(
        &mut self,
        client: &Client,
        secret_key: &str,
        key: &str,
        value: Json,
    ) -> Result<(), AuthError> {
        self.load(client, secret_key).await?;
        if let Some(cache) = self.cache.as_mut() {
            cache.insert(key, value);
        }
        self.modified = true;
        Ok(())
    }

    /// `del session[key]` (`base.py:66-68`).
    pub async fn remove(
        &mut self,
        client: &Client,
        secret_key: &str,
        key: &str,
    ) -> Result<(), AuthError> {
        self.load(client, secret_key).await?;
        if let Some(cache) = self.cache.as_mut() {
            cache.remove(key);
        }
        self.modified = true;
        Ok(())
    }

    /// `set_expiry(seconds)` for an integer (`base.py:343-364`).
    pub async fn set_expiry_seconds(
        &mut self,
        client: &Client,
        secret_key: &str,
        seconds: i64,
    ) -> Result<(), AuthError> {
        self.set(client, secret_key, EXPIRY_SESSION_KEY, Json::Int(seconds))
            .await
    }

    /// `encode()` (`base.py:122-129`).
    pub fn encode(
        &self,
        data: &Json,
        secret_key: &str,
        timestamp: i64,
    ) -> Result<String, AuthError> {
        signing::dumps(data, Self::KEY_SALT, secret_key, timestamp)
            .map_err(|_| AuthError::Database("failed to encode the session".to_string()))
    }

    /// `decode()` (`base.py:131-143`): a bad signature logs
    /// `"Session data corrupted"` to `django.security.SuspiciousSession` and
    /// every other failure returns `{}`. Never an error response.
    pub fn decode(&self, session_data: &str, secret_key: &str) -> Json {
        match signing::loads(session_data, Self::KEY_SALT, secret_key, &[]) {
            Ok(value) if value.as_object().is_some() => value,
            Ok(_) => Json::empty_object(),
            Err(SigningError::BadSignature) => {
                tracing::warn!(
                    target: "django.security.SuspiciousSession",
                    "Session data corrupted"
                );
                Json::empty_object()
            }
            Err(SigningError::Invalid) => Json::empty_object(),
        }
    }

    /// `get_expire_at_browser_close()` (`base.py:366-378`).
    pub async fn get_expire_at_browser_close(
        &mut self,
        client: &Client,
        secret_key: &str,
    ) -> Result<bool, AuthError> {
        match self.get(client, secret_key, EXPIRY_SESSION_KEY).await? {
            None => Ok(SESSION_EXPIRE_AT_BROWSER_CLOSE),
            Some(Json::Int(0)) => Ok(true),
            Some(Json::Float(0.0)) => Ok(true),
            Some(_) => Ok(false),
        }
    }

    /// `get_expiry_age()` (`base.py:265-293`).
    ///
    /// Returns seconds, as a float because a payload may hold one. The
    /// middleware truncates with `int(max_age)` exactly as `Morset.__setitem__`
    /// does for `Max-Age`.
    pub async fn get_expiry_age(
        &mut self,
        client: &Client,
        secret_key: &str,
        settings: &Settings,
    ) -> Result<f64, AuthError> {
        let expiry = self.get(client, secret_key, EXPIRY_SESSION_KEY).await?;
        let now = Utc::now();
        match expiry {
            // `if not expiry:` covers None and 0.
            None => Ok(settings.cookies.session_cookie_age as f64),
            Some(Json::Int(0)) => Ok(settings.cookies.session_cookie_age as f64),
            Some(Json::Float(0.0)) => Ok(settings.cookies.session_cookie_age as f64),
            Some(value) if value.as_i64().is_some() => Ok(value.as_i64().unwrap_or(0) as f64),
            Some(Json::Float(value)) => Ok(value),
            Some(Json::Bool(value)) => Ok(if value { 1.0 } else { 0.0 }),
            Some(Json::Str(text)) => {
                let expiry = parse_python_datetime(&text)?;
                Ok(python_delta_seconds(expiry - now))
            }
            Some(_) => Ok(settings.cookies.session_cookie_age as f64),
        }
    }

    /// `get_expiry_date()` (`base.py:299-321`).
    pub async fn get_expiry_date(
        &mut self,
        client: &Client,
        secret_key: &str,
        settings: &Settings,
    ) -> Result<DateTime<Utc>, AuthError> {
        let expiry = self.get(client, secret_key, EXPIRY_SESSION_KEY).await?;
        let now = Utc::now();
        match expiry {
            Some(Json::Str(text)) => parse_python_datetime(&text),
            Some(Json::Int(0)) | None => {
                Ok(now + TimeDelta::seconds(settings.cookies.session_cookie_age))
            }
            Some(Json::Float(0.0)) => {
                Ok(now + TimeDelta::seconds(settings.cookies.session_cookie_age))
            }
            Some(value) if value.as_i64().is_some() => {
                Ok(now + TimeDelta::seconds(value.as_i64().unwrap_or(0)))
            }
            Some(Json::Float(value)) => Ok(now + TimeDelta::milliseconds((value * 1000.0) as i64)),
            Some(Json::Bool(value)) => Ok(now + TimeDelta::seconds(i64::from(value))),
            Some(_) => Ok(now + TimeDelta::seconds(settings.cookies.session_cookie_age)),
        }
    }

    /// `_get_session(no_load=True)` when there is no key: mark accessed and
    /// cache an empty payload without touching the database.
    ///
    /// `get_user` still has to run `_get_session` (and set `accessed`) for an
    /// anonymous request; skipping the query is what keeps `GET /` working with
    /// Postgres down, exactly like Django's `SimpleLazyObject` never opening a
    /// connection for a request with no session cookie.
    pub fn load_without_database(&mut self) {
        self.accessed = true;
        if self.cache.is_none() {
            self.cache = Some(Json::empty_object());
        }
    }

    /// `exists(session_key)` (`db.py:52-53`).
    pub async fn exists(client: &Client, session_key: &str) -> Result<bool, AuthError> {
        let statement = client.prepare_cached(queries::SESSION_EXISTS.sql).await?;
        Ok(client
            .query_opt(&statement, &[&session_key])
            .await?
            .is_some())
    }

    /// `_get_new_session_key` (`plane/db/models/session.py:35-43`): 128
    /// characters, retried while a row with that key exists.
    async fn new_session_key(client: &Client) -> Result<String, SaveError> {
        for _ in 0..5 {
            let key = get_random_string(128, VALID_KEY_CHARS);
            if !Self::exists(client, &key).await? {
                return Ok(key);
            }
        }
        Err(SaveError::Create)
    }

    /// `create_model_instance` (`plane/db/models/session.py:45-56`).
    fn model_instance(
        &self,
        data: &Json,
        secret_key: &str,
        expire_date: DateTime<Utc>,
    ) -> Result<SessionRow, AuthError> {
        let session_key = self
            .key
            .clone()
            .ok_or_else(|| AuthError::Database("session key missing before insert".to_string()))?;
        let session_data = self.encode(data, secret_key, Utc::now().timestamp())?;
        let user_id = match data.get(SESSION_KEY) {
            None | Some(Json::Null) => None,
            Some(Json::Str(value)) => Some(value.clone()),
            // `obj.user_id = user_id` on a CharField; psycopg would send a
            // non-text parameter and Postgres would reject the insert.
            Some(_) => {
                return Err(AuthError::Database(
                    "session _auth_user_id is not a string".to_string(),
                ));
            }
        };
        let device_info = match data.get("device_info") {
            Some(value @ Json::Object(_)) => Some(value.to_serde_json()),
            _ => None,
        };
        Ok(SessionRow {
            session_key,
            session_data,
            expire_date,
            user_id,
            device_info,
        })
    }

    /// `save(must_create)` (`db.py:117-140`).
    pub async fn save(
        &mut self,
        client: &Client,
        settings: &Settings,
        secret_key: &str,
        must_create: bool,
    ) -> Result<(), SaveError> {
        if self.key.is_none() {
            return self.create(client, settings, secret_key).await;
        }
        self.save_inner(client, settings, secret_key, must_create)
            .await
    }

    /// The insert/update half of `save`, with the key already assigned. Split
    /// out so `create()` is not an async-recursive function.
    async fn save_inner(
        &mut self,
        client: &Client,
        settings: &Settings,
        secret_key: &str,
        must_create: bool,
    ) -> Result<(), SaveError> {
        // `_get_session(no_load=must_create)`: a create never reads the row.
        if self.cache.is_none() {
            if must_create {
                self.accessed = true;
                self.cache = Some(Json::empty_object());
            } else {
                self.load(client, secret_key).await?;
            }
        }
        let data = self.cache.clone().unwrap_or_else(Json::empty_object);
        let expire_date = self.expiry_date_from(&data, settings)?;
        let row = self.model_instance(&data, secret_key, expire_date)?;

        if must_create {
            match client
                .execute(
                    queries::INSERT_SESSION.sql,
                    &[
                        &row.session_key,
                        &row.session_data,
                        &row.expire_date,
                        &row.user_id,
                        &row.device_info,
                    ],
                )
                .await
            {
                Ok(_) => Ok(()),
                Err(error) if is_unique_violation(&error) => Err(SaveError::Create),
                Err(error) => Err(SaveError::from(error)),
            }
        } else {
            let updated = client
                .execute(
                    queries::UPDATE_SESSION.sql,
                    &[
                        &row.session_key,
                        &row.session_data,
                        &row.expire_date,
                        &row.user_id,
                        &row.device_info,
                    ],
                )
                .await;
            match updated {
                // `Model.save(force_update=True)` raises
                // `DatabaseError("Forced update did not affect any rows.")`.
                Ok(0) => Err(SaveError::Update),
                Ok(_) => Ok(()),
                // `except DatabaseError: if not must_create: raise UpdateError`.
                Err(_) => Err(SaveError::Update),
            }
        }
    }

    /// `create()` (`db.py:58-75`).
    async fn create(
        &mut self,
        client: &Client,
        settings: &Settings,
        secret_key: &str,
    ) -> Result<(), SaveError> {
        for _ in 0..5 {
            self.key = Some(Self::new_session_key(client).await?);
            match self.save_inner(client, settings, secret_key, true).await {
                Ok(()) => {
                    self.modified = true;
                    return Ok(());
                }
                Err(SaveError::Create) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(SaveError::Database(AuthError::Database(
            "could not generate a unique session key".to_string(),
        )))
    }

    /// `delete(session_key=None)` (`db.py:142-151`).
    pub async fn delete(&mut self, client: &Client) -> Result<(), AuthError> {
        let Some(key) = self.key.clone() else {
            return Ok(());
        };
        let statement = client.prepare_cached(queries::DELETE_SESSION.sql).await?;
        client.execute(&statement, &[&key]).await?;
        Ok(())
    }

    /// `flush()` (`base.py:408-417`): clear, delete the row, forget the key.
    pub async fn flush(&mut self, client: &Client) -> Result<(), AuthError> {
        self.clear();
        self.delete(client).await?;
        self.key = None;
        Ok(())
    }

    /// `clear()` (`base.py:167-173`): no storage access.
    pub fn clear(&mut self) {
        self.cache = Some(Json::empty_object());
        self.accessed = true;
        self.modified = true;
    }

    /// `cycle_key()` (`base.py:427-438`): a new key with the same data.
    pub async fn cycle_key(
        &mut self,
        client: &Client,
        settings: &Settings,
        secret_key: &str,
    ) -> Result<(), SaveError> {
        self.load(client, secret_key).await?;
        let data = self.cache.clone().unwrap_or_else(Json::empty_object);
        let old_key = self.key.clone();
        self.create(client, settings, secret_key).await?;
        self.cache = Some(data);
        if let Some(old_key) = old_key {
            let statement = client.prepare_cached(queries::DELETE_SESSION.sql).await?;
            client.execute(&statement, &[&old_key]).await?;
        }
        Ok(())
    }

    fn expiry_date_from(
        &self,
        data: &Json,
        settings: &Settings,
    ) -> Result<DateTime<Utc>, AuthError> {
        let now = Utc::now();
        match data.get(EXPIRY_SESSION_KEY) {
            Some(Json::Str(text)) => parse_python_datetime(text),
            Some(Json::Int(value)) if *value != 0 => Ok(now + TimeDelta::seconds(*value)),
            Some(Json::Float(value)) if *value != 0.0 => {
                Ok(now + TimeDelta::milliseconds((value * 1000.0) as i64))
            }
            _ => Ok(now + TimeDelta::seconds(settings.cookies.session_cookie_age)),
        }
    }
}

/// A shared handle to the request's [`SessionStore`].
///
/// Django keeps one `SessionStore` on `request.session`, and both the view and
/// `SessionMiddleware.process_response` use it. Rust's pipeline owns the store
/// in the middleware state, so a handler can only reach it through a handle it
/// shares with that state. `tokio::sync::Mutex` (not `std`) because the guard
/// is held across the database calls `load`/`save`/`flush` make, and the
/// dispatch future must stay `Send`.
///
/// The store is tiny and every operation on it is short, so contention is
/// "the handler flushed while the response phase wanted the flags" — a single
/// request, never two.
#[derive(Debug)]
pub struct SessionSlot {
    store: tokio::sync::Mutex<SessionStore>,
    /// `django.contrib.auth.logout` also replaces `request.user` with
    /// `AnonymousUser` (`django/contrib/auth/__init__.py:243-258`). The slot
    /// records that the store was flushed so the dispatcher can apply the user
    /// half; the store itself has no user.
    logged_out: AtomicBool,
}

impl SessionSlot {
    /// Wrap a store. The middleware calls this once per request.
    pub fn new(store: SessionStore) -> Self {
        Self {
            store: tokio::sync::Mutex::new(store),
            logged_out: AtomicBool::new(false),
        }
    }

    /// Build the layer-3 store from the cookie value for the path.
    pub fn from_cookie(session_key: Option<String>) -> Self {
        Self::new(SessionStore::new(session_key))
    }

    /// Lock the store. Callers must not hold the guard longer than the work
    /// they need it for; the response phase waits for it.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, SessionStore> {
        self.store.lock().await
    }

    /// `request.session.session_key`.
    pub async fn session_key(&self) -> Option<String> {
        self.lock().await.session_key().map(str::to_string)
    }

    /// `request.session.accessed`.
    pub async fn accessed(&self) -> bool {
        self.lock().await.accessed
    }

    /// `request.session.modified`.
    pub async fn modified(&self) -> bool {
        self.lock().await.modified
    }

    /// `request.session.is_empty()`.
    pub async fn is_empty(&self) -> bool {
        self.lock().await.is_empty()
    }

    /// `django.contrib.auth.logout`'s storage half
    /// (`django/contrib/auth/__init__.py:243-258`): `request.session.flush()`,
    /// i.e. `clear()` + `delete()` + `key = None`
    /// (`django/contrib/sessions/backends/base.py:408-417`).
    ///
    /// The response phase then deletes the cookie and adds `Vary: Cookie`
    /// (`plane/authentication/middleware/session.py:53-66`), because the
    /// store is empty and the request carried a cookie.
    pub async fn flush(&self, client: &Client) -> Result<(), AuthError> {
        self.lock().await.flush(client).await?;
        self.logged_out.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Whether [`SessionSlot::flush`] ran (Django's `logout()`).
    pub fn logged_out(&self) -> bool {
        self.logged_out.load(Ordering::SeqCst)
    }
}

/// `Session.objects.filter(user_id=…).delete()` (`plane/app/views/user/base.py:342`):
/// the deactivation path drops **every** session row the user has, including
/// the one serving the request.
///
/// `user_id` is a UUID but the column is `varchar(50)`: Django compares
/// `user_id = str(uuid)`, so the bind is the lower-case dashed text.
pub async fn delete_all_for_user(client: &Client, user_id: Uuid) -> Result<u64, AuthError> {
    let statement = client
        .prepare_cached(queries::DELETE_SESSIONS_FOR_USER.sql)
        .await?;
    Ok(client.execute(&statement, &[&user_id.to_string()]).await?)
}

/// One `sessions` row, exactly the columns Django writes.
#[derive(Debug)]
struct SessionRow {
    session_key: String,
    session_data: String,
    expire_date: DateTime<Utc>,
    user_id: Option<String>,
    device_info: Option<serde_json::Value>,
}

fn is_unique_violation(error: &tokio_postgres::Error) -> bool {
    error
        .code()
        .map(|code| *code == tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
        .unwrap_or(false)
}

/// `datetime.fromisoformat` for the values `set_expiry(datetime)` writes
/// (`value.isoformat()`, always RFC 3339 for an aware datetime).
fn parse_python_datetime(text: &str) -> Result<DateTime<Utc>, AuthError> {
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Ok(value.with_timezone(&Utc));
    }
    // `fromisoformat` also accepts a naive datetime; `USE_TZ=True` makes
    // Django reject one, but parsing it as UTC keeps this total.
    if let Ok(value) = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(value.and_utc());
    }
    Err(AuthError::Database(format!(
        "invalid _session_expiry {text:?}"
    )))
}

/// `delta.days * 86400 + delta.seconds`, which floors a negative delta rather
/// than truncating it toward zero.
fn python_delta_seconds(delta: TimeDelta) -> f64 {
    delta.num_milliseconds().div_euclid(1000) as f64
}

/// What the session middleware must do with the cookie
/// (`plane/authentication/middleware/session.py:60-91`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCookieAction {
    /// `cookie_name in request.COOKIES and empty`: delete the cookie.
    Delete,
    /// `(modified or SESSION_SAVE_EVERY_REQUEST) and not empty`: save and set.
    Save {
        max_age: Option<i64>,
        expires: Option<String>,
    },
    /// Nothing to do.
    None,
}

impl SessionStore {
    /// The response side of `plane/authentication/middleware/session.py`.
    ///
    /// `accessed`, `modified` and `is_empty()` are read *before* any save, as
    /// the Python code does: a load triggered by the expiry computation does
    /// not retroactively add `Vary: Cookie` (Django reads the flag into a local
    /// first, `session.py:53-55`).
    ///
    /// `cookie_sent` is `cookie_name in request.COOKIES`.
    pub async fn response_action(
        &mut self,
        client: &Client,
        settings: &Settings,
        secret_key: &str,
        cookie_sent: bool,
        is_admin_path: bool,
        status_code: u16,
    ) -> Result<SessionCookieAction, SaveError> {
        // The caller reads `self.accessed` before calling this, exactly as
        // `session.py:53-56` snapshots the flag before the save branch.
        let modified = self.modified;
        let empty = self.is_empty();

        if cookie_sent && empty {
            return Ok(SessionCookieAction::Delete);
        }
        if !(modified || settings.cookies.session_save_every_request) || empty {
            return Ok(SessionCookieAction::None);
        }
        let (max_age, expires) = if self.get_expire_at_browser_close(client, secret_key).await? {
            (None, None)
        } else {
            let max_age = if is_admin_path {
                settings.cookies.admin_session_cookie_age
            } else {
                // `int(max_age)` in `Morsel.__setitem__`.
                self.get_expiry_age(client, secret_key, settings).await? as i64
            };
            let expires = http_date(Utc::now().timestamp() + max_age);
            (Some(max_age), Some(expires))
        };
        if status_code < 500 {
            self.save(client, settings, secret_key, false).await?;
            Ok(SessionCookieAction::Save { max_age, expires })
        } else {
            Ok(SessionCookieAction::None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("WEB_URL", "http://localhost")
            .set("SESSION_COOKIE_AGE", "604800")
            .set("ADMIN_SESSION_COOKIE_AGE", "3600");
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    #[test]
    fn key_validation_matches_django() {
        assert!(SessionStore::new(None).session_key().is_none());
        assert!(
            SessionStore::new(Some(String::new()))
                .session_key()
                .is_none()
        );
        assert!(
            SessionStore::new(Some("short".into()))
                .session_key()
                .is_none()
        );
        assert_eq!(
            SessionStore::new(Some("12345678".into())).session_key(),
            Some("12345678")
        );
        let key = "a".repeat(128);
        assert_eq!(
            SessionStore::new(Some(key.clone())).session_key(),
            Some(key.as_str())
        );
    }

    #[test]
    fn is_empty_rules() {
        // No key, no cache → empty; a key alone is not empty; a loaded empty
        // payload is empty, a loaded value is not.
        let mut store = SessionStore::new(None);
        assert!(store.is_empty());
        store.cache = Some(Json::empty_object());
        assert!(store.is_empty());
        store.cache = Some(Json::Object(vec![(
            "_auth_user_id".into(),
            Json::Str("a".into()),
        )]));
        assert!(!store.is_empty());
        let keyed = SessionStore::new(Some("12345678".into()));
        assert!(!keyed.is_empty());
    }

    #[test]
    fn encode_decode_round_trip() {
        let settings = settings();
        let _ = settings;
        let mut store = SessionStore::new(None);
        let data = Json::Object(vec![
            (SESSION_KEY.into(), Json::Str("abc".into())),
            (EXPIRY_SESSION_KEY.into(), Json::Int(3600)),
        ]);
        let encoded = store
            .encode(&data, "secret", 1_777_777_777)
            .expect("encode");
        assert_eq!(store.decode(&encoded, "secret"), data);
        // A wrong secret yields an empty session, never an error.
        assert_eq!(store.decode(&encoded, "other"), Json::empty_object());
        assert_eq!(store.decode("garbage", "secret"), Json::empty_object());
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn expiry_math_matches_django() {
        let settings = settings();
        // no _session_expiry → SESSION_COOKIE_AGE
        let now = Utc::now();
        let mut store = SessionStore::new(None);
        store.cache = Some(Json::empty_object());
        let date = store
            .expiry_date_from(&Json::empty_object(), &settings)
            .expect("date");
        assert!((date - now).num_seconds() - settings.cookies.session_cookie_age < 2);
        // an explicit 3600
        let data = Json::Object(vec![(EXPIRY_SESSION_KEY.into(), Json::Int(3600))]);
        let date = store.expiry_date_from(&data, &settings).expect("date");
        assert!((date - now).num_seconds() - 3600 < 2);
        // zero means browser close, and the expiry date falls back to the age
        let data = Json::Object(vec![(EXPIRY_SESSION_KEY.into(), Json::Int(0))]);
        let date = store.expiry_date_from(&data, &settings).expect("date");
        assert!((date - now).num_seconds() - settings.cookies.session_cookie_age < 2);
        // a datetime string is used verbatim
        let text = (now + TimeDelta::seconds(120)).to_rfc3339();
        let data = Json::Object(vec![(EXPIRY_SESSION_KEY.into(), Json::Str(text))]);
        let date = store.expiry_date_from(&data, &settings).expect("date");
        assert!((date - now).num_seconds() - 120 < 2);
    }

    #[test]
    fn python_delta_floors() {
        assert_eq!(python_delta_seconds(TimeDelta::seconds(5)), 5.0);
        assert_eq!(python_delta_seconds(TimeDelta::milliseconds(-500)), -1.0);
        assert_eq!(python_delta_seconds(TimeDelta::milliseconds(-1500)), -2.0);
        assert_eq!(python_delta_seconds(TimeDelta::milliseconds(1500)), 1.0);
    }

    /// The slot is the one store the handler and the response phase share:
    /// a mutation behind the handle is visible to the next lock, and only
    /// `flush()` marks Django's `logout()`.
    #[tokio::test]
    async fn a_session_slot_shares_one_store() {
        let key = "a".repeat(128);
        let slot = SessionSlot::from_cookie(Some(key.clone()));
        assert_eq!(slot.session_key().await.as_deref(), Some(key.as_str()));
        assert!(!slot.accessed().await);
        assert!(!slot.logged_out());
        {
            let mut store = slot.lock().await;
            store.accessed = true;
            store.modified = true;
        }
        assert!(slot.accessed().await);
        assert!(slot.modified().await);
        assert!(!slot.logged_out());
        // `flush` needs a database for the DELETE; clearing it without one is
        // `SessionStore::flush`'s storage-free half, which is what makes the
        // store empty and the middleware delete the cookie.
        let mut store = slot.lock().await;
        store.clear();
        store.key = None;
        drop(store);
        assert!(slot.is_empty().await);
        assert!(!slot.logged_out());
    }
}
