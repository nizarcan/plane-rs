// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/bgtasks/telemetry_metrics.py:66-381` — the instance metric
//! push to Plane's OTLP collector.
//!
//! # What the Python task does
//!
//! * `Instance.objects.first()` (the soft-delete manager, `-created_at`
//!   ordering) exits silently when there is no row or when
//!   `is_telemetry_enabled` is false (`:74-82`).
//! * It creates a throwaway `MeterProvider` whose resource is
//!   `service.name` = `SERVICE_NAME`, `instance_id`, `plane.instance.type`
//!   (`:88-95`) and registers **fifteen observable gauges**: nine instance
//!   counts and six per-workspace counts (`:145-218`, `:266-343`). The
//!   callbacks close over values read once, before the provider is collected:
//!   the nine `count()` queries at `:115-123`, the first 1000 workspaces by
//!   `created_at` and the six grouped counts at `:224-264`.
//! * `force_flush()` collects and exports once (`:347`); `provider.shutdown()`
//!   in the `finally` wakes the periodic reader, which performs one final
//!   collection and export. **Two identical exports** therefore leave the
//!   process (with different `time_unix_nano` in production), which the
//!   differential harness compares request by request.
//! * `OTLP_METRICS_PROTOCOL` selects gRPC (the default) or HTTP; the HTTP
//!   exporter `POST`s the serialised
//!   `opentelemetry.proto.collector.metrics.v1.ExportMetricsServiceRequest`
//!   protobuf to `<OTLP_ENDPOINT>/v1/metrics`
//!   (`plane/utils/otlp_endpoints.py:56-59`) with
//!   `Content-Type: application/x-protobuf` and
//!   `User-Agent: OTel-OTLP-Exporter-Python/1.45.2`. Transport failures are
//!   retried by the exporter itself (`_OTLPHTTPClient.export`): a fast
//!   connection error is retried immediately once per attempt, then with
//!   `2**retry` seconds of backoff inside a 10 s deadline (four attempts and
//!   eight connections for the dev recorder); a 500 is not retryable
//!   (`429/502/503/504` are). The task itself never retries.
//! * Every exception is swallowed twice: the task body's `except Exception`
//!   (`:360-362`), and the task wrapper's (`:377-381`). A failed push is still
//!   logged as *"Successfully pushed metrics"* because `force_flush()` returns
//!   `True` regardless of the export result — the upstream bug is ported.
//!
//! # The transports
//!
//! `OTLP_METRICS_PROTOCOL` selects gRPC (the default) or HTTP
//! (`telemetry_metrics.py:85-86`): only the exact value `grpc` takes the gRPC
//! branch. Both go through `plane_net::preset::TELEMETRY`, an SSRF-pinned 10 s
//! preset (`DECISIONS.md` D34's amendment), and both are retried by the
//! exporter itself.
//!
//! * **HTTP** (`_OTLPHTTPClient`): `POST <OTLP_ENDPOINT>/v1/metrics` with
//!   `Content-Type: application/x-protobuf` and
//!   `User-Agent: OTel-OTLP-Exporter-Python/1.45.2`. A fast connection error is
//!   retried immediately once per attempt, then with `2**retry` seconds of
//!   backoff inside the deadline; a 500 is not retryable
//!   (`429/502/503/504` are).
//! * **gRPC** (`OTLPExporterMixin`, `opentelemetry/exporter/otlp/proto/grpc/
//!   exporter.py`): one unary `MetricsService/Export` over HTTP/2 with the
//!   gRPC frame, `content-type: application/grpc`, `te: trailers`, the C-core
//!   user agent and metadata, and `grpc-status` trailers. Retryable codes are
//!   CANCELLED, DEADLINE_EXCEEDED, RESOURCE_EXHAUSTED, ABORTED, OUT_OF_RANGE,
//!   UNAVAILABLE and DATA_LOSS, six attempts at most, `2**retry` backoff inside
//!   the deadline. A transport failure is UNAVAILABLE; a local deadline is
//!   DEADLINE_EXCEEDED. TLS uses the pinned certifi roots with ALPN `h2`;
//!   `OTEL_EXPORTER_OTLP_METRICS_INSECURE=true` selects plaintext h2c.
//!
//! `OTEL_EXPORTER_OTLP_METRICS_{HEADERS,COMPRESSION}` (and their global
//! `OTEL_EXPORTER_OTLP_*` fallbacks) apply to both transports; gzip bodies are
//! compressed with `flate2` (the gzip header bytes are not byte-identical to
//! Python's `gzip.GzipFile`, which embeds the mtime). The exporter timeout
//! (`OTEL_EXPORTER_OTLP_METRICS_TIMEOUT`, else the global, else 10 s) is the
//! retry deadline. `OTLP_ENDPOINT` is the documented override; `difftest task`
//! points it at its loopback recorder with `--telemetry-url '{{http}}'` (HTTP)
//! or a case `grpc` block (gRPC).
//!
//! # Byte-identical body
//!
//! The protobuf is written by hand, field by field. The parts that vary are
//! the collection timestamp (`time_unix_nano`, one value for every data point
//! in one collection) and `service.instance.id` (a per-process uuid4 that the
//! `OTEL_RESOURCE_ATTRIBUTES` detector can override). The harness pins both:
//! `OTEL_RESOURCE_ATTRIBUTES=service.instance.id=…` and
//! `PLANE_RS_DIFFTEST_METRICS_NS=<ns>` (only honoured in a `difftest` build,
//! like `PLANE_RS_DIFFTEST_ALLOW_LOOPBACK`).

use std::io::Write as _;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use deadpool_postgres::Client;
use plane_core::config::Settings;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_net::grpc::{STATUS_DEADLINE_EXCEEDED, STATUS_UNAVAILABLE};
use plane_net::{FetchError, Method, OutboundRequest, SsrfPolicy, preset};
use uuid::Uuid;

use crate::{TaskContext, swallow};

/// The Celery task name (`telemetry_metrics.py:369`).
pub const TASK: &str = "plane.license.bgtasks.telemetry_metrics.push_instance_metrics";

/// `provider.get_meter(__name__)` (`telemetry_metrics.py:112`).
const SCOPE_NAME: &str = "plane.license.bgtasks.telemetry_metrics";

/// `WORKSPACE_METRICS_LIMIT` (`telemetry_metrics.py:36`).
const WORKSPACE_METRICS_LIMIT: i64 = 1000;

/// `_OTLPHTTPClient`'s default export timeout
/// (`opentelemetry/exporter/otlp/common/http.py`, `_DEFAULT_TIMEOUT = 10.0`).
const DEFAULT_EXPORT_TIMEOUT: Duration = Duration::from_secs(10);

/// `_OTLPHTTPClient`'s `_MAX_RETRIES`; the gRPC mixin's `_MAX_RETRYS` is the
/// same number with a different spelling.
const MAX_RETRIES: u32 = 6;

/// `MetricsServiceStub.Export`'s full method path.
const GRPC_PATH: &str = "/opentelemetry.proto.collector.metrics.v1.MetricsService/Export";

/// `_RETRYABLE_ERROR_CODES` from
/// `opentelemetry/exporter/otlp/proto/grpc/exporter.py`: CANCELLED,
/// DEADLINE_EXCEEDED, RESOURCE_EXHAUSTED, ABORTED, OUT_OF_RANGE, UNAVAILABLE,
/// DATA_LOSS.
const GRPC_RETRYABLE: [u32; 7] = [1, 4, 8, 10, 11, 14, 15];

/// The nine instance gauges, in registration order (`telemetry_metrics.py:175-218`).
const INSTANCE_METRICS: [(&str, &str); 9] = [
    (
        "plane_instance_users_total",
        "Total number of users in the Plane instance",
    ),
    (
        "plane_instance_workspaces_total",
        "Total number of workspaces",
    ),
    (
        "plane_instance_projects_total",
        "Total number of projects across all workspaces",
    ),
    (
        "plane_instance_issues_total",
        "Total number of issues across all projects",
    ),
    ("plane_instance_modules_total", "Total number of modules"),
    ("plane_instance_cycles_total", "Total number of cycles"),
    (
        "plane_instance_cycle_issues_total",
        "Total number of issues in cycles",
    ),
    (
        "plane_instance_module_issues_total",
        "Total number of issues in modules",
    ),
    ("plane_instance_pages_total", "Total number of pages"),
];

/// The six workspace gauges, in registration order (`telemetry_metrics.py:314-343`).
const WORKSPACE_METRICS: [(&str, &str); 6] = [
    (
        "plane_workspace_projects_total",
        "Number of projects per workspace",
    ),
    (
        "plane_workspace_issues_total",
        "Number of issues per workspace",
    ),
    (
        "plane_workspace_modules_total",
        "Number of modules per workspace",
    ),
    (
        "plane_workspace_cycles_total",
        "Number of cycles per workspace",
    ),
    (
        "plane_workspace_members_total",
        "Number of members per workspace",
    ),
    (
        "plane_workspace_pages_total",
        "Number of pages per workspace",
    ),
];

/// Register the handler; the worker's registry owns the closure.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| handle(Arc::clone(&context), job));
}

/// `Instance.objects.first()`'s fields that change the body (`:115-143`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct InstanceMetrics {
    instance_id: String,
    instance_name: String,
    current_version: String,
    latest_version: String,
    edition: String,
    is_telemetry_enabled: bool,
    is_verified: bool,
    is_setup_done: bool,
}

/// The nine instance-level counts (`:115-123`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    users: i64,
    workspaces: i64,
    projects: i64,
    issues: i64,
    modules: i64,
    cycles: i64,
    cycle_issues: i64,
    module_issues: i64,
    pages: i64,
}

impl Counts {
    fn values(self) -> [i64; 9] {
        [
            self.users,
            self.workspaces,
            self.projects,
            self.issues,
            self.modules,
            self.cycles,
            self.cycle_issues,
            self.module_issues,
            self.pages,
        ]
    }
}

/// One workspace's six counts (`:266-279`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct WorkspaceCounts {
    projects: i64,
    issues: i64,
    modules: i64,
    cycles: i64,
    members: i64,
    pages: i64,
}

impl WorkspaceCounts {
    fn values(self) -> [i64; 6] {
        [
            self.projects,
            self.issues,
            self.modules,
            self.cycles,
            self.members,
            self.pages,
        ]
    }
}

/// Everything one push needs, read before any request is made.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    instance: InstanceMetrics,
    domain: String,
    counts: Counts,
    /// `Workspace.objects.order_by("created_at")[:1000]`, in that order.
    workspaces: Vec<(Uuid, String, WorkspaceCounts)>,
}

/// The selected exporter transport and its configuration, built before the
/// database reads exactly where `_create_otlp_metric_exporter()` runs in
/// Python (`telemetry_metrics.py:99-108`, outside the task body's `try`).
#[derive(Debug, Clone)]
enum Transport {
    Http {
        url: String,
        headers: Vec<(String, String)>,
        compression: Option<HttpCompression>,
    },
    Grpc {
        endpoint: String,
        insecure: bool,
        metadata: Vec<(String, String)>,
        compression: Option<&'static str>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HttpCompression {
    Gzip,
    Deflate,
}

impl Transport {
    fn from_settings(settings: &Settings) -> Result<Self, HandlerError> {
        let headers = exporter_headers();
        if settings.telemetry.otlp_metrics_protocol == "grpc" {
            Ok(Self::Grpc {
                endpoint: grpc_endpoint(&settings.telemetry.otlp_endpoint)?,
                insecure: grpc_insecure(),
                metadata: grpc_metadata(headers),
                compression: grpc_compression()?,
            })
        } else {
            Ok(Self::Http {
                url: metrics_url(&settings.telemetry.otlp_endpoint),
                compression: http_compression(),
                headers,
            })
        }
    }

    fn describe(&self) -> (&str, &str) {
        match self {
            Self::Http { url, .. } => ("http", url),
            Self::Grpc { endpoint, .. } => ("grpc", endpoint),
        }
    }

    /// One exporter attempt (`MetricExporter.export`), including the
    /// transport's own retries.
    async fn send(&self, context: &TaskContext, body: &[u8]) -> bool {
        match self {
            Self::Http {
                url,
                headers,
                compression,
            } => {
                let mut headers = headers.clone();
                let encoded = match compression {
                    Some(compression) => match compress_http(*compression, body) {
                        Ok(encoded) => {
                            if !headers
                                .iter()
                                .any(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
                            {
                                headers.push((
                                    "Content-Encoding".to_string(),
                                    match compression {
                                        HttpCompression::Gzip => "gzip",
                                        HttpCompression::Deflate => "deflate",
                                    }
                                    .to_string(),
                                ));
                            }
                            encoded
                        }
                        Err(error) => {
                            tracing::warn!(target: "plane.tasks", %error, "compressing the metrics body failed");
                            return false;
                        }
                    },
                    None => body.to_vec(),
                };
                export_http(context, url, &headers, &encoded).await
            }
            Self::Grpc {
                endpoint,
                insecure,
                metadata,
                compression,
            } => {
                let encoded = match compression {
                    Some("gzip") => match compress_gzip(body) {
                        Ok(encoded) => encoded,
                        Err(error) => {
                            tracing::warn!(target: "plane.tasks", %error, "compressing the metrics body failed");
                            return false;
                        }
                    },
                    _ => body.to_vec(),
                };
                export_grpc(
                    context,
                    endpoint,
                    *insecure,
                    metadata,
                    &encoded,
                    *compression,
                )
                .await
            }
        }
    }
}

async fn handle(context: Arc<TaskContext>, job: JobContext) -> Result<(), HandlerError> {
    // `telemetry_metrics.py:360-362` and `:377-381`: log and succeed.
    if let Err(error) = run(&context, &job.client).await {
        swallow(error, TASK);
    }
    Ok(())
}

/// The task body. Every failure is the Python `except Exception` branch.
async fn run(context: &TaskContext, client: &Client) -> Result<(), HandlerError> {
    let Some(instance) = load_instance(client).await? else {
        tracing::debug!(target: "plane.tasks", "No instance registered, skipping metrics push");
        return Ok(());
    };
    if !instance.is_telemetry_enabled {
        tracing::debug!(target: "plane.tasks", "Telemetry disabled, skipping metrics push");
        return Ok(());
    }
    // `:85-108`: the exporter (and its channel/endpoint validation) is built
    // before the counts and outside the body's `try`.
    let transport = Transport::from_settings(&context.settings)?;
    let domain = domain(context.settings.urls.web_url.as_deref())?;
    let counts = instance_counts(client).await?;
    let resource = resource_attributes(
        &context.settings.telemetry.service_name,
        &instance.instance_id,
        std::env::var("OTEL_RESOURCE_ATTRIBUTES").ok().as_deref(),
        std::env::var("OTEL_SERVICE_NAME").ok().as_deref(),
    );
    let disabled = otel_sdk_disabled();
    let workspaces = match workspace_metrics(client).await {
        Ok(workspaces) => workspaces,
        Err(error) => {
            // At this point the nine instance gauges are registered and the six
            // workspace gauges are not (`:174-218` precedes `:224-264`), so the
            // task's `finally: provider.shutdown()` exports the instance-only
            // body once before the exception is logged.
            if !disabled {
                let partial = Snapshot {
                    instance: instance.clone(),
                    domain,
                    counts,
                    workspaces: Vec::new(),
                };
                let body = encode(&partial, &resource, now_ns());
                let _ = transport.send(context, &body).await;
            }
            return Err(error);
        }
    };
    let snapshot = Snapshot {
        domain,
        counts,
        workspaces,
        instance,
    };
    // `OTEL_SDK_DISABLED`: `get_meter` returns a `NoOpMeter`, so the values are
    // still read but the reader collects nothing and no request is made.
    if disabled {
        tracing::debug!(target: "plane.tasks", "OTEL_SDK_DISABLED is true, skipping metrics push");
        return Ok(());
    }

    // `force_flush()` (`:347`) and then `provider.shutdown()`'s final
    // collection (`:365`): two exports of the same captured values with two
    // collection timestamps.
    for _ in 0..2 {
        let body = encode(&snapshot, &resource, now_ns());
        let _ = transport.send(context, &body).await;
    }

    // `flush_success` is always true, so the info line is logged even when the
    // export failed (`:349-353`).
    let (kind, endpoint) = transport.describe();
    tracing::info!(
        target: "plane.tasks",
        transport = kind,
        endpoint = %endpoint,
        instance = %snapshot.instance.instance_id,
        "Successfully pushed metrics to OTEL collector"
    );
    Ok(())
}

/// `Instance.objects.first()` (`:74`).
async fn load_instance(client: &Client) -> Result<Option<InstanceMetrics>, HandlerError> {
    let row = client
        .query_opt(
            "SELECT instance_id, instance_name, current_version, latest_version, edition, \
                    is_telemetry_enabled, is_verified, is_setup_done \
             FROM instances WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
            &[],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(InstanceMetrics {
        instance_id: row.try_get(0).map_err(db)?,
        instance_name: row.try_get(1).map_err(db)?,
        current_version: row.try_get(2).map_err(db)?,
        // Nullable columns: `str(instance.latest_version or "")`.
        latest_version: row
            .try_get::<_, Option<String>>(3)
            .map_err(db)?
            .unwrap_or_default(),
        edition: row.try_get(4).map_err(db)?,
        is_telemetry_enabled: row.try_get(5).map_err(db)?,
        is_verified: row.try_get(6).map_err(db)?,
        is_setup_done: row.try_get(7).map_err(db)?,
    }))
}

async fn count(client: &Client, sql: &str) -> Result<i64, HandlerError> {
    client
        .query_one(sql, &[])
        .await
        .map_err(db)?
        .try_get(0)
        .map_err(db)
}

/// The nine `count()` queries, one per table, with the managers' filters
/// (`:115-123`). `User` has no `deleted_at` column.
async fn instance_counts(client: &Client) -> Result<Counts, HandlerError> {
    Ok(Counts {
        users: count(client, "SELECT count(*) FROM users WHERE NOT is_bot").await?,
        workspaces: count(
            client,
            "SELECT count(*) FROM workspaces WHERE deleted_at IS NULL",
        )
        .await?,
        projects: count(
            client,
            "SELECT count(*) FROM projects WHERE deleted_at IS NULL",
        )
        .await?,
        issues: count(
            client,
            "SELECT count(*) FROM issues WHERE deleted_at IS NULL",
        )
        .await?,
        modules: count(
            client,
            "SELECT count(*) FROM modules WHERE deleted_at IS NULL",
        )
        .await?,
        cycles: count(
            client,
            "SELECT count(*) FROM cycles WHERE deleted_at IS NULL",
        )
        .await?,
        cycle_issues: count(
            client,
            "SELECT count(*) FROM cycle_issues WHERE deleted_at IS NULL",
        )
        .await?,
        module_issues: count(
            client,
            "SELECT count(*) FROM module_issues WHERE deleted_at IS NULL",
        )
        .await?,
        // `Page.objects.exclude(owned_by__is_bot=True, access=1)`: an INNER
        // JOIN on the non-null `owned_by` and `NOT (access = 1 AND is_bot)`.
        pages: count(
            client,
            "SELECT count(*) FROM pages \
             INNER JOIN users ON pages.owned_by_id = users.id \
             WHERE pages.deleted_at IS NULL AND NOT (pages.access = 1 AND users.is_bot)",
        )
        .await?,
    })
}

/// The workspace list and the six grouped counts (`:224-264`).
async fn workspace_metrics(
    client: &Client,
) -> Result<Vec<(Uuid, String, WorkspaceCounts)>, HandlerError> {
    let rows = client
        .query(
            "SELECT id, slug FROM workspaces WHERE deleted_at IS NULL \
             ORDER BY created_at ASC LIMIT $1",
            &[&WORKSPACE_METRICS_LIMIT],
        )
        .await
        .map_err(db)?;
    let mut workspaces: Vec<(Uuid, String)> = rows
        .iter()
        .map(|row| Ok((row.try_get(0).map_err(db)?, row.try_get(1).map_err(db)?)))
        .collect::<Result<_, HandlerError>>()?;
    if workspaces.is_empty() {
        // Django compiles `workspace_id__in=[]` to `EmptyResultSet` and never
        // reaches the database; nothing is learned from the six queries.
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = workspaces.iter().map(|(id, _)| *id).collect();

    let project = grouped_counts(
        client,
        "SELECT workspace_id, count(id) FROM projects \
         WHERE deleted_at IS NULL AND workspace_id = ANY($1::uuid[]) GROUP BY 1",
        &ids,
    )
    .await?;
    let issue = grouped_counts(
        client,
        "SELECT workspace_id, count(id) FROM issues \
         WHERE deleted_at IS NULL AND workspace_id = ANY($1::uuid[]) GROUP BY 1",
        &ids,
    )
    .await?;
    let module = grouped_counts(
        client,
        "SELECT workspace_id, count(id) FROM modules \
         WHERE deleted_at IS NULL AND workspace_id = ANY($1::uuid[]) GROUP BY 1",
        &ids,
    )
    .await?;
    let cycle = grouped_counts(
        client,
        "SELECT workspace_id, count(id) FROM cycles \
         WHERE deleted_at IS NULL AND workspace_id = ANY($1::uuid[]) GROUP BY 1",
        &ids,
    )
    .await?;
    let member = grouped_counts(
        client,
        "SELECT workspace_id, count(id) FROM workspace_members \
         WHERE deleted_at IS NULL AND workspace_id = ANY($1::uuid[]) GROUP BY 1",
        &ids,
    )
    .await?;
    let page = grouped_counts(
        client,
        "SELECT pages.workspace_id, count(pages.id) FROM pages \
         INNER JOIN users ON pages.owned_by_id = users.id \
         WHERE pages.deleted_at IS NULL AND pages.workspace_id = ANY($1::uuid[]) \
           AND NOT (pages.access = 1 AND users.is_bot) GROUP BY 1",
        &ids,
    )
    .await?;

    Ok(workspaces
        .drain(..)
        .map(|(id, slug)| {
            let counts = WorkspaceCounts {
                projects: project.get(&id).copied().unwrap_or(0),
                issues: issue.get(&id).copied().unwrap_or(0),
                modules: module.get(&id).copied().unwrap_or(0),
                cycles: cycle.get(&id).copied().unwrap_or(0),
                members: member.get(&id).copied().unwrap_or(0),
                pages: page.get(&id).copied().unwrap_or(0),
            };
            (id, slug, counts)
        })
        .collect())
}

async fn grouped_counts(
    client: &Client,
    sql: &str,
    ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, i64>, HandlerError> {
    let rows = client.query(sql, &[&ids]).await.map_err(db)?;
    rows.iter()
        .map(|row| Ok((row.try_get(0).map_err(db)?, row.try_get(1).map_err(db)?)))
        .collect()
}

/// `WEB_URL` → `urlparse(...).netloc` (`:125-131`).
fn domain(web_url: Option<&str>) -> Result<String, HandlerError> {
    let raw = web_url.unwrap_or("");
    if raw.is_empty() {
        return Ok(String::new());
    }
    let with_scheme;
    let url = if raw.contains("://") {
        raw
    } else {
        // `"//" + web_url` so `urlparse` treats the host as a host.
        with_scheme = format!("//{raw}");
        &with_scheme
    };
    // `urlparse` raises `ValueError` on an invalid IPv6 literal; the task's
    // outer `except Exception` then skips the push.
    plane_core::py::urlsplit(url)
        .map(|parsed| parsed.netloc)
        .map_err(|error| HandlerError::retryable(error.to_string()))
}

/// `get_otlp_http_metrics_url()` (`plane/utils/otlp_endpoints.py:56-59`).
fn metrics_url(endpoint: &str) -> String {
    format!("{}/v1/metrics", endpoint.trim_end_matches('/'))
}

fn db<E: std::fmt::Display>(error: E) -> HandlerError {
    HandlerError::retryable(error.to_string())
}

fn telemetry_policy() -> SsrfPolicy {
    #[cfg(feature = "difftest")]
    if std::env::var("PLANE_RS_DIFFTEST_ALLOW_LOOPBACK").as_deref() == Ok("1") {
        return SsrfPolicy::new(
            vec![
                "127.0.0.0/8".parse().expect("loopback v4 network"),
                "::1/128".parse().expect("loopback v6 network"),
            ],
            std::iter::empty(),
        );
    }
    SsrfPolicy::strict()
}

/// `provider.force_flush()`'s collection timestamp (`time_ns()`).
fn now_ns() -> u64 {
    #[cfg(feature = "difftest")]
    if let Ok(raw) = std::env::var("PLANE_RS_DIFFTEST_METRICS_NS")
        && let Ok(value) = raw.trim().parse::<u64>()
    {
        return value;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0)
}

/// `OTEL_SDK_DISABLED` (`MeterProvider.__init__`): `lower().strip() == "true"`.
fn otel_sdk_disabled() -> bool {
    std::env::var("OTEL_SDK_DISABLED").is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

/// `_resolve_timeout(OTEL_EXPORTER_OTLP_METRICS_TIMEOUT)`:
/// `<signal timeout> or OTEL_EXPORTER_OTLP_TIMEOUT or 10`, parsed as a float.
fn export_timeout() -> Duration {
    let raw = std::env::var("OTEL_EXPORTER_OTLP_METRICS_TIMEOUT")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("OTEL_EXPORTER_OTLP_TIMEOUT")
                .ok()
                .filter(|value| !value.is_empty())
        });
    let Some(raw) = raw else {
        return DEFAULT_EXPORT_TIMEOUT;
    };
    match raw.parse::<f64>() {
        Ok(seconds) if seconds.is_finite() && seconds >= 0.0 => Duration::from_secs_f64(seconds),
        _ => {
            tracing::warn!(
                target: "plane.tasks",
                raw,
                "invalid OTLP timeout; using the 10 s default"
            );
            DEFAULT_EXPORT_TIMEOUT
        }
    }
}

/// `_OTLPHTTPClient.export`: two connection attempts per `_submit` (the
/// immediate retry on a connection error), then `2**retry` seconds of backoff
/// while the 10 s deadline lasts. Jitter is omitted: it only moves the sleeps,
/// never the number of attempts inside the deadline.
async fn export_http(
    context: &TaskContext,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> bool {
    let deadline = Instant::now() + export_timeout();
    let mut retry = 0;
    loop {
        let mut backoff = Duration::from_secs(1u64 << retry.min(20));
        match submit_http(context, url, headers, body, deadline).await {
            Ok(response) => {
                if (200..400).contains(&response.status) {
                    return true;
                }
                let retryable = matches!(response.status, 429 | 502 | 503 | 504);
                if !retryable {
                    tracing::warn!(
                        target: "plane.tasks",
                        status = response.status,
                        "Failed to export metrics batch"
                    );
                    return false;
                }
                if let Some(after) = response.header("retry-after").and_then(retry_after) {
                    backoff = after;
                }
            }
            Err(error) => {
                tracing::warn!(target: "plane.tasks", %error, "Failed to export metrics batch");
                // `is_connection_error` covers `requests.exceptions`'
                // connection/TLS/timeout classes, which `plane-net` returns as
                // `Transport`. A `ValueError` (URL validation) or
                // `InvalidURL` (preparation) is not retried by the exporter.
                if !matches!(error, FetchError::Transport(_)) {
                    return false;
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if retry + 1 == MAX_RETRIES || backoff > remaining {
            return false;
        }
        tokio::time::sleep(backoff).await;
        retry += 1;
    }
}

async fn submit_http(
    context: &TaskContext,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    deadline: Instant,
) -> Result<plane_net::PinnedResponse, FetchError> {
    let mut request = OutboundRequest::new().body(body.to_vec());
    for (name, value) in headers {
        request = request.header(name.clone(), value.clone());
    }
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(1));
    let mut telemetry = preset::TELEMETRY;
    telemetry.timeout = Some(remaining);
    match context
        .http
        .fetch_once(&telemetry, Method::POST, &telemetry_policy(), url, &request)
        .await
    {
        Ok(response) => Ok(response),
        Err(first) => {
            // `_submit` retries a connection error once, immediately.
            if !matches!(first, FetchError::Transport(_)) {
                return Err(first);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(first);
            }
            let mut telemetry = preset::TELEMETRY;
            telemetry.timeout = Some(remaining);
            context
                .http
                .fetch_once(&telemetry, Method::POST, &telemetry_policy(), url, &request)
                .await
        }
    }
}

/// `OTLPExporterMixin._export` for the gRPC exporter: `UNKNOWN` from a
/// transport failure, then the retry table and `2**retry` backoff inside the
/// `_timeout` deadline.
async fn export_grpc(
    context: &TaskContext,
    endpoint: &str,
    insecure: bool,
    metadata: &[(String, String)],
    body: &[u8],
    compression: Option<&'static str>,
) -> bool {
    let deadline = Instant::now() + export_timeout();
    let mut retry = 0;
    loop {
        let backoff = Duration::from_secs(1u64 << retry.min(20));
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let request = plane_net::grpc::GrpcRequest {
            endpoint,
            path: GRPC_PATH,
            insecure,
            metadata: metadata.to_vec(),
            body: body.to_vec(),
            compression,
            timeout: remaining,
        };
        let code = match context
            .http
            .grpc_unary(&preset::TELEMETRY, &telemetry_policy(), &request)
            .await
        {
            Ok(response) => {
                if response.status.code == 0 {
                    return true;
                }
                if response.status.code == STATUS_DEADLINE_EXCEEDED {
                    tracing::warn!(
                        target: "plane.tasks",
                        "gRPC export exceeded its deadline"
                    );
                } else {
                    tracing::warn!(
                        target: "plane.tasks",
                        status = response.status.code,
                        message = response.status.message.as_deref().unwrap_or(""),
                        "Failed to export metrics batch"
                    );
                }
                response.status.code
            }
            Err(error) => {
                tracing::warn!(target: "plane.tasks", %error, "Failed to export metrics batch");
                if !matches!(error, FetchError::Grpc(_)) {
                    return false;
                }
                STATUS_UNAVAILABLE
            }
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !GRPC_RETRYABLE.contains(&code) || retry + 1 == MAX_RETRIES || backoff > remaining {
            return false;
        }
        tokio::time::sleep(backoff).await;
        retry += 1;
    }
}

/// `OTEL_EXPORTER_OTLP_METRICS_HEADERS` (else `OTEL_EXPORTER_OTLP_HEADERS`).
fn exporter_headers() -> Vec<(String, String)> {
    let raw = non_empty_env("OTEL_EXPORTER_OTLP_METRICS_HEADERS")
        .or_else(|| non_empty_env("OTEL_EXPORTER_OTLP_HEADERS"));
    raw.map(|raw| parse_headers(&raw)).unwrap_or_default()
}

/// `parse_env_headers(..., liberal=True)`: comma-separated `name=value`,
/// URL-decoded, names lower-cased, values stripped.
fn parse_headers(raw: &str) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    for item in raw.split(',') {
        if item.is_empty() {
            continue;
        }
        let Some((name, value)) = item.split_once('=') else {
            tracing::warn!(
                target: "plane.tasks",
                header = item,
                "invalid OTLP header; expected name=value"
            );
            continue;
        };
        let name = unquote(name).trim().to_lowercase();
        if name.is_empty() {
            continue;
        }
        upsert(&mut headers, &name, unquote(value).trim().to_string());
    }
    headers
}

/// `grpc` metadata adds the exporter's primary user agent to the channel's own
/// `grpc-python/<version> grpc-c/<version> (<platform>; chttp2)`.
fn grpc_metadata(mut headers: Vec<(String, String)>) -> Vec<(String, String)> {
    upsert(&mut headers, "user-agent", grpc_user_agent());
    headers
}

/// `grpcio==1.84.0` (gRPC C-core 56.0.0) in the oracle virtualenv; the
/// channel option `grpc.primary_user_agent` prefixes
/// `OTel-OTLP-Exporter-Python/1.45.2`. Bump both pinned versions with
/// `apps/api/pyproject.toml`.
fn grpc_user_agent() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "osx",
        other => other,
    };
    format!(
        "OTel-OTLP-Exporter-Python/1.45.2 grpc-python/1.84.0 grpc-c/56.0.0 ({platform}; chttp2)"
    )
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// `environ_to_compression(OTEL_EXPORTER_OTLP_METRICS_COMPRESSION)` for the
/// gRPC exporter. A metrics value of `none` is `None`, which the mixin treats
/// as "not provided" and re-reads the global variable.
fn grpc_compression() -> Result<Option<&'static str>, HandlerError> {
    if let Some(value) = non_empty_env("OTEL_EXPORTER_OTLP_METRICS_COMPRESSION") {
        match value.trim().to_lowercase().as_str() {
            "gzip" => return Ok(Some("gzip")),
            "none" => {}
            other => {
                return Err(HandlerError::retryable(format!(
                    "invalid value {other:?} for OTEL_EXPORTER_OTLP_METRICS_COMPRESSION"
                )));
            }
        }
    }
    match non_empty_env("OTEL_EXPORTER_OTLP_COMPRESSION") {
        Some(value) => match value.trim().to_lowercase().as_str() {
            "gzip" => Ok(Some("gzip")),
            "none" => Ok(None),
            other => Err(HandlerError::retryable(format!(
                "invalid value {other:?} for OTEL_EXPORTER_OTLP_COMPRESSION"
            ))),
        },
        None => Ok(None),
    }
}

/// `_resolve_compression` for the HTTP exporter: an invalid value warns and
/// falls back to no compression.
fn http_compression() -> Option<HttpCompression> {
    let raw = non_empty_env("OTEL_EXPORTER_OTLP_METRICS_COMPRESSION")
        .or_else(|| non_empty_env("OTEL_EXPORTER_OTLP_COMPRESSION"));
    match raw.as_deref().map(|value| value.trim().to_lowercase()) {
        Some(value) if value == "gzip" => Some(HttpCompression::Gzip),
        Some(value) if value == "deflate" => Some(HttpCompression::Deflate),
        Some(value) if value == "none" => None,
        Some(other) => {
            tracing::warn!(
                target: "plane.tasks",
                value = other,
                "unsupported OTLP compression; sending uncompressed"
            );
            None
        }
        None => None,
    }
}

fn compress_http(compression: HttpCompression, body: &[u8]) -> Result<Vec<u8>, HandlerError> {
    match compression {
        HttpCompression::Gzip => compress_gzip(body),
        HttpCompression::Deflate => {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder
                .write_all(body)
                .and_then(|()| encoder.finish())
                .map_err(|error| HandlerError::retryable(error.to_string()))
        }
    }
}

fn compress_gzip(body: &[u8]) -> Result<Vec<u8>, HandlerError> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(body)
        .and_then(|()| encoder.finish())
        .map_err(|error| HandlerError::retryable(error.to_string()))
}

/// `OTEL_EXPORTER_OTLP_METRICS_INSECURE`: the task passes
/// `insecure=os.environ.get(..., "").lower() == "true"`, so the endpoint's
/// scheme is not consulted (the gRPC endpoint is always `host:port`).
fn grpc_insecure() -> bool {
    std::env::var("OTEL_EXPORTER_OTLP_METRICS_INSECURE")
        .is_ok_and(|value| value.to_lowercase() == "true")
}

/// `get_otlp_grpc_endpoint()` (`plane/utils/otlp_endpoints.py:46-53`): a
/// scheme-less `host:port` with 443 for `https` and OTLP's 4317 otherwise.
fn grpc_endpoint(raw: &str) -> Result<String, HandlerError> {
    let url = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("//{raw}")
    };
    let parsed = plane_core::py::urlsplit(&url)
        .map_err(|error| HandlerError::retryable(error.to_string()))?;
    let host = parsed
        .hostname()
        .unwrap_or_else(|| "telemetry.plane.so".to_string());
    let port = match parsed
        .port()
        .map_err(|error| HandlerError::retryable(error.to_string()))?
    {
        Some(port) => port.to_string(),
        None if parsed.scheme == "https" => "443".to_string(),
        None => "4317".to_string(),
    };
    Ok(format!("{host}:{port}"))
}

/// `Retry-After`'s delay-seconds form. Python also accepts an HTTP date;
/// nothing sets one (the OTLP retry set is 429/502/503/504).
fn retry_after(value: &str) -> Option<Duration> {
    let seconds = value.trim().parse::<f64>().ok()?;
    if !seconds.is_finite() {
        return None;
    }
    Some(Duration::from_secs_f64(seconds.max(0.0)))
}

/// `Resource.create({...})`'s attribute list, in Python dict order
/// (`opentelemetry/sdk/resources/__init__.py:199-205, 291-299, 330-361`):
/// the SDK defaults, the random `service.instance.id`, then
/// `OTEL_RESOURCE_ATTRIBUTES` (which can override it in place),
/// `OTEL_SERVICE_NAME`, and finally the task's own attributes.
fn resource_attributes(
    service_name: &str,
    instance_id: &str,
    otel_resource_attributes: Option<&str>,
    otel_service_name: Option<&str>,
) -> Vec<(String, String)> {
    let mut attributes = vec![
        ("telemetry.sdk.language".to_string(), "python".to_string()),
        (
            "telemetry.sdk.name".to_string(),
            "opentelemetry".to_string(),
        ),
        ("telemetry.sdk.version".to_string(), "1.45.2".to_string()),
        (
            "service.instance.id".to_string(),
            process_instance_id().to_string(),
        ),
    ];
    for (key, value) in parse_resource_attributes(otel_resource_attributes.unwrap_or("")) {
        upsert(&mut attributes, &key, value);
    }
    if let Some(name) = otel_service_name.filter(|value| !value.is_empty()) {
        upsert(&mut attributes, "service.name", name.to_string());
    }
    upsert(&mut attributes, "service.name", service_name.to_string());
    upsert(&mut attributes, "instance_id", instance_id.to_string());
    upsert(
        &mut attributes,
        "plane.instance.type",
        "self-hosted".to_string(),
    );
    attributes
}

/// The SDK's per-process uuid4 (`ServiceInstanceIdResourceDetector`).
fn process_instance_id() -> &'static str {
    static ID: LazyLock<String> = LazyLock::new(|| Uuid::new_v4().to_string());
    &ID
}

/// `OTELResourceDetector.detect`'s comma-separated `key=value` list.
fn parse_resource_attributes(raw: &str) -> Vec<(String, String)> {
    let mut attributes = Vec::new();
    for item in raw.split(',') {
        let Some((key, value)) = item.split_once('=') else {
            if !item.trim().is_empty() {
                tracing::warn!(
                    target: "plane.tasks",
                    item,
                    "invalid OTEL_RESOURCE_ATTRIBUTES pair"
                );
            }
            continue;
        };
        attributes.push((key.trim().to_string(), unquote(value.trim())));
    }
    attributes
}

/// Python dict update semantics: an existing key keeps its position.
fn upsert(attributes: &mut Vec<(String, String)>, key: &str, value: String) {
    if let Some(existing) = attributes.iter_mut().find(|(name, _)| name == key) {
        existing.1 = value;
    } else {
        attributes.push((key.to_string(), value));
    }
}

/// `urllib.parse.unquote` (UTF-8, `errors="replace"`; bad escapes are kept).
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Protobuf encoding
// ---------------------------------------------------------------------------

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// A length-delimited field (wire type 2).
fn field_bytes(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    varint(out, u64::from(field) << 3 | 2);
    varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

/// A fixed64 field (wire type 1).
fn field_fixed64(out: &mut Vec<u8>, field: u32, value: u64) {
    varint(out, u64::from(field) << 3 | 1);
    out.extend_from_slice(&value.to_le_bytes());
}

/// `opentelemetry.proto.common.v1.AnyValue{string_value}`.
fn any_string(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    field_bytes(&mut out, 1, value.as_bytes());
    out
}

/// `opentelemetry.proto.common.v1.KeyValue`.
fn key_value(key: &str, value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    field_bytes(&mut out, 1, key.as_bytes());
    field_bytes(&mut out, 2, &any_string(value));
    out
}

/// The `KeyValue` messages of a `repeated KeyValue attributes` field.
fn attribute_entries(attributes: &[(String, String)]) -> Vec<Vec<u8>> {
    attributes
        .iter()
        .map(|(key, value)| key_value(key, value))
        .collect()
}

/// `opentelemetry.proto.metrics.v1.Metric` with a `Gauge` body.
fn metric(name: &str, description: &str, points: &[Vec<u8>]) -> Vec<u8> {
    let mut gauge = Vec::new();
    for point in points {
        field_bytes(&mut gauge, 1, point);
    }
    let mut out = Vec::new();
    field_bytes(&mut out, 1, name.as_bytes());
    field_bytes(&mut out, 2, description.as_bytes());
    // field 5 = gauge (`unit` is empty and omitted)
    field_bytes(&mut out, 5, &gauge);
    out
}

/// `opentelemetry.proto.metrics.v1.NumberDataPoint` for an integer gauge.
///
/// Field order is protobuf serialisation order: time (3), value (6),
/// attributes (7). `value` is a `oneof`, so `as_int = 0` is the selected
/// member and **is** written (unlike a plain proto3 default).
fn number_data_point(attributes: &[(String, String)], timestamp_ns: u64, value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    field_fixed64(&mut out, 3, timestamp_ns);
    field_fixed64(&mut out, 6, value as u64);
    for entry in attribute_entries(attributes) {
        field_bytes(&mut out, 7, &entry);
    }
    out
}

/// The whole `ExportMetricsServiceRequest`.
fn encode(snapshot: &Snapshot, resource: &[(String, String)], timestamp_ns: u64) -> Vec<u8> {
    let instance_attributes = [
        ("instance_id", snapshot.instance.instance_id.as_str()),
        ("instance_name", snapshot.instance.instance_name.as_str()),
        (
            "current_version",
            snapshot.instance.current_version.as_str(),
        ),
        ("latest_version", snapshot.instance.latest_version.as_str()),
        ("edition", snapshot.instance.edition.as_str()),
        ("domain", snapshot.domain.as_str()),
        (
            "is_verified",
            if snapshot.instance.is_verified {
                "true"
            } else {
                "false"
            },
        ),
        (
            "is_setup_done",
            if snapshot.instance.is_setup_done {
                "true"
            } else {
                "false"
            },
        ),
    ];
    let instance_attributes: Vec<(String, String)> = instance_attributes
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();

    let mut metrics = Vec::new();
    for ((name, description), value) in INSTANCE_METRICS.iter().zip(snapshot.counts.values()) {
        let point = number_data_point(&instance_attributes, timestamp_ns, value);
        metrics.push(metric(name, description, &[point]));
    }
    for (index, (name, description)) in WORKSPACE_METRICS.iter().enumerate() {
        let points: Vec<Vec<u8>> = snapshot
            .workspaces
            .iter()
            .map(|(id, slug, counts)| {
                let value = counts.values()[index];
                number_data_point(
                    &[
                        ("workspace_id".to_string(), id.to_string()),
                        ("workspace_slug".to_string(), slug.clone()),
                        (
                            "instance_id".to_string(),
                            snapshot.instance.instance_id.clone(),
                        ),
                    ],
                    timestamp_ns,
                    value,
                )
            })
            .collect();
        // A metric whose callback yields nothing has no data points and is
        // dropped by the SDK (`MetricReaderStorage.collect` skips it), so the
        // encoder never sees an empty gauge.
        if !points.is_empty() {
            metrics.push(metric(name, description, &points));
        }
    }

    let mut scope = Vec::new();
    field_bytes(&mut scope, 1, SCOPE_NAME.as_bytes());

    let mut scope_metrics = Vec::new();
    field_bytes(&mut scope_metrics, 1, &scope);
    for metric in &metrics {
        field_bytes(&mut scope_metrics, 2, metric);
    }

    let mut resource_message = Vec::new();
    for entry in attribute_entries(resource) {
        field_bytes(&mut resource_message, 1, &entry);
    }
    let mut resource_metrics = Vec::new();
    field_bytes(&mut resource_metrics, 1, &resource_message);
    field_bytes(&mut resource_metrics, 2, &scope_metrics);

    let mut request = Vec::new();
    field_bytes(&mut request, 1, &resource_metrics);
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 5 210 bytes Django's exporter writes for
    /// `tools/jobs/fixtures/t-golden.py` (a standalone copy of the metric
    /// registration with the values below, `OTEL_RESOURCE_ATTRIBUTES`
    /// `service.instance.id=00000000-0000-4000-8000-000000000000` and
    /// `time_ns()` frozen at 1 700 000 000 123 456 789 on
    /// `opentelemetry-sdk==1.45.2`).
    const GOLDEN_HEX: &str = concat!(
        "0ad7280a8c020a220a1674656c656d657472792e73646b2e6c616e677561676512080a06707974686f6e0a250a127465",
        "6c656d657472792e73646b2e6e616d65120f0a0d6f70656e74656c656d657472790a210a1574656c656d657472792e73",
        "646b2e76657273696f6e12080a06312e34352e320a3d0a13736572766963652e696e7374616e63652e696412260a2430",
        "303030303030302d303030302d343030302d383030302d3030303030303030303030300a1e0a0c736572766963652e6e",
        "616d65120e0a0c706c616e652d63652d6170690a170a0b696e7374616e63655f696412080a066162633132330a240a13",
        "706c616e652e696e7374616e63652e74797065120d0a0b73656c662d686f7374656412c5260a290a27706c616e652e6c",
        "6963656e73652e62677461736b732e74656c656d657472795f6d65747269637312cd020a1a706c616e655f696e737461",
        "6e63655f75736572735f746f74616c122b546f74616c206e756d626572206f6620757365727320696e2074686520506c",
        "616e6520696e7374616e63652a81020afe011915cd853dfe9c97173107000000000000003a170a0b696e7374616e6365",
        "5f696412080a066162633132333a2a0a0d696e7374616e63655f6e616d6512190a17506c616e6520436f6d6d756e6974",
        "792045646974696f6e3a1b0a0f63757272656e745f76657273696f6e12080a0676312e352e303a1a0a0e6c6174657374",
        "5f76657273696f6e12080a0676312e352e313a1c0a0765646974696f6e12110a0f504c414e455f434f4d4d554e495459",
        "3a1d0a06646f6d61696e12130a11706c616e652e6578616d706c652e636f6d3a150a0b69735f76657269666965641206",
        "0a04747275653a180a0d69735f73657475705f646f6e6512070a0566616c736512c1020a1f706c616e655f696e737461",
        "6e63655f776f726b7370616365735f746f74616c121a546f74616c206e756d626572206f6620776f726b737061636573",
        "2a81020afe011915cd853dfe9c97173102000000000000003a170a0b696e7374616e63655f696412080a066162633132",
        "333a2a0a0d696e7374616e63655f6e616d6512190a17506c616e6520436f6d6d756e6974792045646974696f6e3a1b0a",
        "0f63757272656e745f76657273696f6e12080a0676312e352e303a1a0a0e6c61746573745f76657273696f6e12080a06",
        "76312e352e313a1c0a0765646974696f6e12110a0f504c414e455f434f4d4d554e4954593a1d0a06646f6d61696e1213",
        "0a11706c616e652e6578616d706c652e636f6d3a150a0b69735f766572696669656412060a04747275653a180a0d6973",
        "5f73657475705f646f6e6512070a0566616c736512d3020a1d706c616e655f696e7374616e63655f70726f6a65637473",
        "5f746f74616c122e546f74616c206e756d626572206f662070726f6a65637473206163726f737320616c6c20776f726b",
        "7370616365732a81020afe011915cd853dfe9c97173103000000000000003a170a0b696e7374616e63655f696412080a",
        "066162633132333a2a0a0d696e7374616e63655f6e616d6512190a17506c616e6520436f6d6d756e6974792045646974",
        "696f6e3a1b0a0f63757272656e745f76657273696f6e12080a0676312e352e303a1a0a0e6c61746573745f7665727369",
        "6f6e12080a0676312e352e313a1c0a0765646974696f6e12110a0f504c414e455f434f4d4d554e4954593a1d0a06646f",
        "6d61696e12130a11706c616e652e6578616d706c652e636f6d3a150a0b69735f766572696669656412060a0474727565",
        "3a180a0d69735f73657475705f646f6e6512070a0566616c736512cd020a1b706c616e655f696e7374616e63655f6973",
        "737565735f746f74616c122a546f74616c206e756d626572206f6620697373756573206163726f737320616c6c207072",
        "6f6a656374732a81020afe011915cd853dfe9c97173104000000000000003a170a0b696e7374616e63655f696412080a",
        "066162633132333a2a0a0d696e7374616e63655f6e616d6512190a17506c616e6520436f6d6d756e6974792045646974",
        "696f6e3a1b0a0f63757272656e745f76657273696f6e12080a0676312e352e303a1a0a0e6c61746573745f7665727369",
        "6f6e12080a0676312e352e313a1c0a0765646974696f6e12110a0f504c414e455f434f4d4d554e4954593a1d0a06646f",
        "6d61696e12130a11706c616e652e6578616d706c652e636f6d3a150a0b69735f766572696669656412060a0474727565",
        "3a180a0d69735f73657475705f646f6e6512070a0566616c736512bb020a1c706c616e655f696e7374616e63655f6d6f",
        "64756c65735f746f74616c1217546f74616c206e756d626572206f66206d6f64756c65732a81020afe011915cd853dfe",
        "9c97173101000000000000003a170a0b696e7374616e63655f696412080a066162633132333a2a0a0d696e7374616e63",
        "655f6e616d6512190a17506c616e6520436f6d6d756e6974792045646974696f6e3a1b0a0f63757272656e745f766572",
        "73696f6e12080a0676312e352e303a1a0a0e6c61746573745f76657273696f6e12080a0676312e352e313a1c0a076564",
        "6974696f6e12110a0f504c414e455f434f4d4d554e4954593a1d0a06646f6d61696e12130a11706c616e652e6578616d",
        "706c652e636f6d3a150a0b69735f766572696669656412060a04747275653a180a0d69735f73657475705f646f6e6512",
        "070a0566616c736512b9020a1b706c616e655f696e7374616e63655f6379636c65735f746f74616c1216546f74616c20",
        "6e756d626572206f66206379636c65732a81020afe011915cd853dfe9c97173102000000000000003a170a0b696e7374",
        "616e63655f696412080a066162633132333a2a0a0d696e7374616e63655f6e616d6512190a17506c616e6520436f6d6d",
        "756e6974792045646974696f6e3a1b0a0f63757272656e745f76657273696f6e12080a0676312e352e303a1a0a0e6c61",
        "746573745f76657273696f6e12080a0676312e352e313a1c0a0765646974696f6e12110a0f504c414e455f434f4d4d55",
        "4e4954593a1d0a06646f6d61696e12130a11706c616e652e6578616d706c652e636f6d3a150a0b69735f766572696669",
        "656412060a04747275653a180a0d69735f73657475705f646f6e6512070a0566616c736512c9020a21706c616e655f69",
        "6e7374616e63655f6379636c655f6973737565735f746f74616c1220546f74616c206e756d626572206f662069737375",
        "657320696e206379636c65732a81020afe011915cd853dfe9c97173103000000000000003a170a0b696e7374616e6365",
        "5f696412080a066162633132333a2a0a0d696e7374616e63655f6e616d6512190a17506c616e6520436f6d6d756e6974",
        "792045646974696f6e3a1b0a0f63757272656e745f76657273696f6e12080a0676312e352e303a1a0a0e6c6174657374",
        "5f76657273696f6e12080a0676312e352e313a1c0a0765646974696f6e12110a0f504c414e455f434f4d4d554e495459",
        "3a1d0a06646f6d61696e12130a11706c616e652e6578616d706c652e636f6d3a150a0b69735f76657269666965641206",
        "0a04747275653a180a0d69735f73657475705f646f6e6512070a0566616c736512cb020a22706c616e655f696e737461",
        "6e63655f6d6f64756c655f6973737565735f746f74616c1221546f74616c206e756d626572206f662069737375657320",
        "696e206d6f64756c65732a81020afe011915cd853dfe9c97173104000000000000003a170a0b696e7374616e63655f69",
        "6412080a066162633132333a2a0a0d696e7374616e63655f6e616d6512190a17506c616e6520436f6d6d756e69747920",
        "45646974696f6e3a1b0a0f63757272656e745f76657273696f6e12080a0676312e352e303a1a0a0e6c61746573745f76",
        "657273696f6e12080a0676312e352e313a1c0a0765646974696f6e12110a0f504c414e455f434f4d4d554e4954593a1d",
        "0a06646f6d61696e12130a11706c616e652e6578616d706c652e636f6d3a150a0b69735f766572696669656412060a04",
        "747275653a180a0d69735f73657475705f646f6e6512070a0566616c736512b7020a1a706c616e655f696e7374616e63",
        "655f70616765735f746f74616c1215546f74616c206e756d626572206f662070616765732a81020afe011915cd853dfe",
        "9c97173105000000000000003a170a0b696e7374616e63655f696412080a066162633132333a2a0a0d696e7374616e63",
        "655f6e616d6512190a17506c616e6520436f6d6d756e6974792045646974696f6e3a1b0a0f63757272656e745f766572",
        "73696f6e12080a0676312e352e303a1a0a0e6c61746573745f76657273696f6e12080a0676312e352e313a1c0a076564",
        "6974696f6e12110a0f504c414e455f434f4d4d554e4954593a1d0a06646f6d61696e12130a11706c616e652e6578616d",
        "706c652e636f6d3a150a0b69735f766572696669656412060a04747275653a180a0d69735f73657475705f646f6e6512",
        "070a0566616c736512c3020a1e706c616e655f776f726b73706163655f70726f6a656374735f746f74616c12204e756d",
        "626572206f662070726f6a656374732070657220776f726b73706163652afe010a7d1915cd853dfe9c97173102000000",
        "000000003a360a0c776f726b73706163655f696412260a2431313131313131312d313131312d343131312d383131312d",
        "3131313131313131313131313a180a0e776f726b73706163655f736c756712060a0461636d653a170a0b696e7374616e",
        "63655f696412080a066162633132330a7d1915cd853dfe9c97173100000000000000003a360a0c776f726b7370616365",
        "5f696412260a2432323232323232322d323232322d343232322d383232322d3232323232323232323232323a180a0e77",
        "6f726b73706163655f736c756712060a04626574613a170a0b696e7374616e63655f696412080a0661626331323312bf",
        "020a1c706c616e655f776f726b73706163655f6973737565735f746f74616c121e4e756d626572206f66206973737565",
        "732070657220776f726b73706163652afe010a7d1915cd853dfe9c97173103000000000000003a360a0c776f726b7370",
        "6163655f696412260a2431313131313131312d313131312d343131312d383131312d3131313131313131313131313a18",
        "0a0e776f726b73706163655f736c756712060a0461636d653a170a0b696e7374616e63655f696412080a066162633132",
        "330a7d1915cd853dfe9c97173100000000000000003a360a0c776f726b73706163655f696412260a2432323232323232",
        "322d323232322d343232322d383232322d3232323232323232323232323a180a0e776f726b73706163655f736c756712",
        "060a04626574613a170a0b696e7374616e63655f696412080a0661626331323312c1020a1d706c616e655f776f726b73",
        "706163655f6d6f64756c65735f746f74616c121f4e756d626572206f66206d6f64756c65732070657220776f726b7370",
        "6163652afe010a7d1915cd853dfe9c97173100000000000000003a360a0c776f726b73706163655f696412260a243131",
        "3131313131312d313131312d343131312d383131312d3131313131313131313131313a180a0e776f726b73706163655f",
        "736c756712060a0461636d653a170a0b696e7374616e63655f696412080a066162633132330a7d1915cd853dfe9c9717",
        "3100000000000000003a360a0c776f726b73706163655f696412260a2432323232323232322d323232322d343232322d",
        "383232322d3232323232323232323232323a180a0e776f726b73706163655f736c756712060a04626574613a170a0b69",
        "6e7374616e63655f696412080a0661626331323312bf020a1c706c616e655f776f726b73706163655f6379636c65735f",
        "746f74616c121e4e756d626572206f66206379636c65732070657220776f726b73706163652afe010a7d1915cd853dfe",
        "9c97173101000000000000003a360a0c776f726b73706163655f696412260a2431313131313131312d313131312d3431",
        "31312d383131312d3131313131313131313131313a180a0e776f726b73706163655f736c756712060a0461636d653a17",
        "0a0b696e7374616e63655f696412080a066162633132330a7d1915cd853dfe9c97173100000000000000003a360a0c77",
        "6f726b73706163655f696412260a2432323232323232322d323232322d343232322d383232322d323232323232323232",
        "3232323a180a0e776f726b73706163655f736c756712060a04626574613a170a0b696e7374616e63655f696412080a06",
        "61626331323312c1020a1d706c616e655f776f726b73706163655f6d656d626572735f746f74616c121f4e756d626572",
        "206f66206d656d626572732070657220776f726b73706163652afe010a7d1915cd853dfe9c9717310400000000000000",
        "3a360a0c776f726b73706163655f696412260a2431313131313131312d313131312d343131312d383131312d31313131",
        "31313131313131313a180a0e776f726b73706163655f736c756712060a0461636d653a170a0b696e7374616e63655f69",
        "6412080a066162633132330a7d1915cd853dfe9c97173101000000000000003a360a0c776f726b73706163655f696412",
        "260a2432323232323232322d323232322d343232322d383232322d3232323232323232323232323a180a0e776f726b73",
        "706163655f736c756712060a04626574613a170a0b696e7374616e63655f696412080a0661626331323312bd020a1b70",
        "6c616e655f776f726b73706163655f70616765735f746f74616c121d4e756d626572206f662070616765732070657220",
        "776f726b73706163652afe010a7d1915cd853dfe9c97173102000000000000003a360a0c776f726b73706163655f6964",
        "12260a2431313131313131312d313131312d343131312d383131312d3131313131313131313131313a180a0e776f726b",
        "73706163655f736c756712060a0461636d653a170a0b696e7374616e63655f696412080a066162633132330a7d1915cd",
        "853dfe9c97173100000000000000003a360a0c776f726b73706163655f696412260a2432323232323232322d32323232",
        "2d343232322d383232322d3232323232323232323232323a180a0e776f726b73706163655f736c756712060a04626574",
        "613a170a0b696e7374616e63655f696412080a06616263313233"
    );

    fn golden_snapshot() -> Snapshot {
        Snapshot {
            instance: InstanceMetrics {
                instance_id: "abc123".to_string(),
                instance_name: "Plane Community Edition".to_string(),
                current_version: "v1.5.0".to_string(),
                latest_version: "v1.5.1".to_string(),
                edition: "PLANE_COMMUNITY".to_string(),
                is_telemetry_enabled: true,
                is_verified: true,
                is_setup_done: false,
            },
            domain: "plane.example.com".to_string(),
            counts: Counts {
                users: 7,
                workspaces: 2,
                projects: 3,
                issues: 4,
                modules: 1,
                cycles: 2,
                cycle_issues: 3,
                module_issues: 4,
                pages: 5,
            },
            workspaces: vec![
                (
                    Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("uuid"),
                    "acme".to_string(),
                    WorkspaceCounts {
                        projects: 2,
                        issues: 3,
                        modules: 0,
                        cycles: 1,
                        members: 4,
                        pages: 2,
                    },
                ),
                (
                    Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("uuid"),
                    "beta".to_string(),
                    WorkspaceCounts {
                        projects: 0,
                        issues: 0,
                        modules: 0,
                        cycles: 0,
                        members: 1,
                        pages: 0,
                    },
                ),
            ],
        }
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        clean
            .as_bytes()
            .chunks(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex"), 16).expect("byte")
            })
            .collect()
    }

    #[test]
    fn encodes_the_python_golden_body() {
        let snapshot = golden_snapshot();
        let resource = resource_attributes(
            "plane-ce-api",
            "abc123",
            Some("service.instance.id=00000000-0000-4000-8000-000000000000"),
            None,
        );
        let body = encode(&snapshot, &resource, 1_700_000_000_123_456_789);
        let golden = hex_bytes(GOLDEN_HEX);
        if body != golden {
            let first = body
                .iter()
                .zip(&golden)
                .position(|(left, right)| left != right);
            panic!(
                "body differs (rust {} bytes, django {} bytes, first {first:?})\nrust:  {:02x?}\ndjango:{:02x?}",
                body.len(),
                golden.len(),
                &body[..body.len().min(64)],
                &golden[..golden.len().min(64)],
            );
        }
    }

    #[test]
    fn workspace_metrics_are_omitted_when_no_workspace_exists() {
        let mut snapshot = golden_snapshot();
        snapshot.workspaces.clear();
        let body = encode(&snapshot, &[], 1);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("plane_instance_users_total"));
        assert!(!text.contains("plane_workspace_projects_total"));
        assert!(!text.contains("plane_workspace_pages_total"));
    }

    #[test]
    fn the_resource_attributes_keep_python_dict_order() {
        let attributes = resource_attributes("plane-ce-api", "abc123", None, None);
        let keys: Vec<&str> = attributes.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "telemetry.sdk.language",
                "telemetry.sdk.name",
                "telemetry.sdk.version",
                "service.instance.id",
                "service.name",
                "instance_id",
                "plane.instance.type",
            ]
        );

        // An env override keeps the detector's position.
        let attributes = resource_attributes(
            "plane-ce-api",
            "abc123",
            Some(
                "service.instance.id=11111111-1111-4111-8111-111111111111,deployment.environment=prod",
            ),
            Some("ignored"),
        );
        let keys: Vec<&str> = attributes.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "telemetry.sdk.language",
                "telemetry.sdk.name",
                "telemetry.sdk.version",
                "service.instance.id",
                "deployment.environment",
                "service.name",
                "instance_id",
                "plane.instance.type",
            ]
        );
        assert_eq!(attributes[3].1, "11111111-1111-4111-8111-111111111111");
        // The task's explicit service.name wins over OTEL_SERVICE_NAME.
        assert_eq!(attributes[5].1, "plane-ce-api");
    }

    #[test]
    fn grpc_endpoint_matches_get_otlp_grpc_endpoint() {
        assert_eq!(
            grpc_endpoint("https://telemetry.plane.so").expect("endpoint"),
            "telemetry.plane.so:443"
        );
        assert_eq!(
            grpc_endpoint("telemetry.plane.so:4317").expect("endpoint"),
            "telemetry.plane.so:4317"
        );
        assert_eq!(
            grpc_endpoint("telemetry.plane.so").expect("endpoint"),
            "telemetry.plane.so:4317"
        );
        assert_eq!(
            grpc_endpoint("http://collector").expect("endpoint"),
            "collector:4317"
        );
        assert_eq!(
            grpc_endpoint("https://collector:8443").expect("endpoint"),
            "collector:8443"
        );
        // `urlsplit` keeps the IPv6 hostname unbracketed, so Python's
        // `f"{host}:{port}"` produces this malformed value; port it.
        assert_eq!(
            grpc_endpoint("https://[::1]:4317").expect("endpoint"),
            "::1:4317"
        );
        assert_eq!(
            grpc_endpoint("").expect("endpoint"),
            "telemetry.plane.so:4317"
        );
    }

    #[test]
    fn headers_are_parsed_like_parse_env_headers() {
        assert_eq!(
            parse_headers("x-test=hello%20world, X-Other = two ,empty=,dup=1,dup=2"),
            [
                ("x-test".to_string(), "hello world".to_string()),
                ("x-other".to_string(), "two".to_string()),
                ("empty".to_string(), String::new()),
                ("dup".to_string(), "2".to_string()),
            ]
        );
        assert!(parse_headers("").is_empty());
    }

    #[test]
    fn compression_round_trips() {
        use std::io::Read as _;

        let body = b"metrics".repeat(100);
        let gzip = compress_gzip(&body).expect("gzip");
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(gzip.as_slice())
            .read_to_end(&mut decoded)
            .expect("gunzip");
        assert_eq!(decoded, body);

        let deflate = compress_http(HttpCompression::Deflate, &body).expect("deflate");
        let mut decoded = Vec::new();
        flate2::read::ZlibDecoder::new(deflate.as_slice())
            .read_to_end(&mut decoded)
            .expect("inflate");
        assert_eq!(decoded, body);
    }

    #[test]
    fn grpc_user_agent_is_the_python_core_string() {
        let agent = grpc_user_agent();
        assert!(
            agent
                .starts_with("OTel-OTLP-Exporter-Python/1.45.2 grpc-python/1.84.0 grpc-c/56.0.0 (")
        );
        assert!(agent.ends_with("; chttp2)"));
        let platform = if cfg!(target_os = "macos") {
            "osx"
        } else {
            std::env::consts::OS
        };
        assert!(agent.contains(&format!("({platform};")), "{agent}");
    }

    #[test]
    fn resource_attribute_pairs_are_unquoted_like_python() {
        assert_eq!(
            parse_resource_attributes("a=one%20two,b=100%25,c=bad%zz"),
            [
                ("a".to_string(), "one two".to_string()),
                ("b".to_string(), "100%".to_string()),
                ("c".to_string(), "bad%zz".to_string()),
            ]
        );
        assert_eq!(unquote("%C3%A9"), "é");
    }

    #[test]
    fn domain_uses_urlparse() {
        assert_eq!(
            domain(Some("https://plane.acmecorp.com")).expect("domain"),
            "plane.acmecorp.com"
        );
        assert_eq!(
            domain(Some("plane.acmecorp.com")).expect("domain"),
            "plane.acmecorp.com"
        );
        assert_eq!(
            domain(Some("plane.example.com:8443/x")).expect("domain"),
            "plane.example.com:8443"
        );
        assert_eq!(domain(Some("")).expect("domain"), "");
        assert_eq!(domain(None).expect("domain"), "");
        // `urlparse` rejects an unbalanced IPv6 literal; the task's outer
        // `except Exception` swallows it and sends nothing.
        assert!(domain(Some("http://[::1")).is_err());
    }

    #[test]
    fn metrics_url_appends_the_signal_path() {
        assert_eq!(
            metrics_url("https://telemetry.plane.so"),
            "https://telemetry.plane.so/v1/metrics"
        );
        assert_eq!(
            metrics_url("https://telemetry.plane.so///"),
            "https://telemetry.plane.so/v1/metrics"
        );
        assert_eq!(metrics_url(""), "/v1/metrics");
    }

    #[test]
    fn retry_after_accepts_seconds_only() {
        assert_eq!(retry_after(" 2 "), Some(Duration::from_secs(2)));
        assert_eq!(retry_after("-1"), Some(Duration::ZERO));
        assert_eq!(retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
    }

    #[test]
    fn varints_match_the_protobuf_encoding() {
        let mut out = Vec::new();
        varint(&mut out, 0);
        varint(&mut out, 1);
        varint(&mut out, 300);
        varint(&mut out, u64::MAX);
        assert_eq!(
            out,
            [
                0x00, 0x01, 0xac, 0x02, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01
            ]
        );
    }
}
