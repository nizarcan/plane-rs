# Helm Chart: Plane Community (Rust backend)

The chart lives in this repository at [`charts/plane-rs`](../../../charts/plane-rs).
It deploys the Rust backend (`plane-api`, `plane-worker`, `plane-beat`,
`plane-migrator`) and the `web`, `space`, `admin` and `live` frontends, with
optional bundled PostgreSQL, Valkey and MinIO, optional ingress, and
`existingSecret` hooks for External Secrets Operator. See the
[chart README](../../../charts/plane-rs/README.md) for the full values table.

Quick install:

```bash
helm install plane-rs charts/plane-rs \
  --namespace plane-ce --create-namespace \
  --set secrets.app.secretKey="$(openssl rand -hex 32)" \
  --set secrets.app.liveServerSecretKey="$(openssl rand -hex 32)" \
  --set ingress.enabled=true \
  --set ingress.className=nginx \
  --set ingress.host=plane.example.com
```

## Images

Images are published to GHCR by `.github/workflows/build-branch.yml`, multi-arch
(`linux/amd64`, `linux/arm64`), with the same release tag for every component:

| Component | Image |
| --- | --- |
| api | `ghcr.io/nizarcan/plane-rs-api` |
| worker | `ghcr.io/nizarcan/plane-rs-worker` |
| beat | `ghcr.io/nizarcan/plane-rs-beat` |
| migrator | `ghcr.io/nizarcan/plane-rs-migrator` |
| web | `ghcr.io/nizarcan/plane-rs-web` |
| space | `ghcr.io/nizarcan/plane-rs-space` |
| admin | `ghcr.io/nizarcan/plane-rs-admin` |
| live | `ghcr.io/nizarcan/plane-rs-live` |
| proxy | `ghcr.io/nizarcan/plane-rs-proxy` (Caddy, for the compose stacks) |

## Phase 8 cutover: what changed from the `plane-ce` chart

This replaces the old chart note (images `makeplane/plane-*`, one
`plane-backend` Deployment running `./bin/docker-entrypoint-{api,worker,beat,migrator}.sh`):

1. **Four backend images, no command override.** `plane-api` (HTTP, port 8000),
   `plane-worker` (the `plane_jobs` worker), `plane-beat` (the scheduler) and
   `plane-migrator` (the one-shot schema owner). The images are distroless: no
   shell, no `exec` probes, no `collectstatic`, no static volume.
2. **Migrator as a Job.** `pre-install,pre-upgrade` hooks (ArgoCD: `PreSync`)
   when the database is external and its Secret already exists; otherwise a
   regular Job whose marker the api/worker/beat wait for
   (`wait_for_migrations`). The Job is named from a checksum of its image,
   config and database inputs, not `.Release.Revision` (ArgoCD renders with
   `helm template`, where the revision is always 1), so a new version creates a
   new Job and an unchanged render keeps the completed one. A baseline refusal
   fails the release. There is no `manage.py migrate` any more.
3. **No RabbitMQ.** The broker, its StatefulSet/Service/PVC and the
   `rabbitmq.*` values are gone; `AMQP_URL`, `RABBITMQ_*`, `CELERY_*`,
   `GUNICORN_WORKERS` and `METRICS_PUSH_INTERVAL_MINUTES` are not rendered.
   The queue is the `plane_jobs` schema in the same PostgreSQL database.
   `PLANE_RS_JOB_CONCURRENCY` (4), `PLANE_RS_JOB_LEASE_SECONDS` (60),
   `PLANE_RS_JOB_DRAIN_SECONDS` (30) and `PLANE_RS_JOB_POLL_MS` (1000) are the
   tuning surface.
4. **Same environment file everywhere.** The worker and beat pods read the same
   ConfigMap/Secrets as the api: `SECRET_KEY` (instance-configuration
   decryption), `DATABASE_URL`, `REDIS_URL`, `AWS_*`, `WEB_URL`, `LIVE_URL`,
   `WEBHOOK_ALLOWED_IPS`, `WEBHOOK_ALLOWED_HOSTS`.
5. **Proxy/Ingress.** `/api/*`, `/auth/*` and `/static/*` go to `plane-api:8000`
   (`/static/*` is the api's embedded copy of the eight files `collectstatic`
   used to publish). `/spaces/*`, `/god-mode/*`, `/live/*` and `/uploads/*`
   (bundled MinIO) and `/` route as in `apps/proxy/Caddyfile.ce`. The
   coexistence Caddyfile, `route-groups.yaml` and `PLANE_RS_GROUP_*` are
   dev/harness-only and are not in the chart.
6. **Probes.** The api answers `GET /` with `{"status": "OK"}` without touching
   Postgres, so it is readiness, liveness and (with a five-minute window) the
   startup probe. Worker and beat have no HTTP listener and no shell for an exec
   probe: they carry no probe and rely on the process exiting on a fatal error.
   Live uses a TCP probe because a plain GET returns 404. Logs go to stdout;
   there are no `/code/plane/logs` volumes.
7. **PVCs/backups.** No RabbitMQ PVC. PostgreSQL is the only stateful workload
   that matters for a restore (`django_celery_beat_*` tables stay in the
   baseline, empty and unused). MinIO holds uploads; keep both.

See `apps/server/DECISIONS.md` D34 and D36 for the authoritative decisions, and
`apps/server/tools/cutover/exit_check.py` for the fresh-install/upgrade walk
the chart is expected to satisfy.
