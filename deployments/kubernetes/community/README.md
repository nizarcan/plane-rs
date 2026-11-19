# Helm Chart: Plane Community

Click on the below link to access the helm chart instructions.

[![Artifact Hub](https://img.shields.io/endpoint?url=https://artifacthub.io/badge/repository/makeplane)](https://artifacthub.io/packages/helm/makeplane/plane-ce)

Quick install:

```bash
helm repo add makeplane https://helm.plane.so/
helm repo update
helm install plane makeplane/plane-ce --namespace plane-ce --create-namespace
```

See the [Kubernetes self-hosting guide](https://developers.plane.so/self-hosting/methods/kubernetes) for the values you need to set (domain, ingress, external services).

## Phase 8 cutover: chart changes (not in this repository)

The Helm chart lives in `makeplane/helm-charts`; this note is the specification
for the release that ships the Rust backend. The compose stacks in this
repository are the reference implementation (`docker-compose.yml` and
`deployments/cli/community/docker-compose.yml`).

1. **Images.** The single `plane-backend` deployment that runs
   `./bin/docker-entrypoint-{api,worker,beat,migrator}.sh` becomes four images
   with no `command:` override:
   - `makeplane/plane-api` — the HTTP server (port 8000),
   - `makeplane/plane-worker` — the `plane_jobs` worker,
   - `makeplane/plane-beat` — the scheduler,
   - `makeplane/plane-migrator` — the one-shot schema owner.
   Use the same release tag as the frontends. The images are distroless: no
   shell, no `exec` probes, no `collectstatic`.
2. **Migrator.** Run `plane-migrator` as a `pre-install,pre-upgrade` Job (and
   as the release's schema step) and wait for `condition=complete` before the
   api/worker/beat Deployments roll. The binaries themselves block in
   `wait_for_migrations` until the migrator writes its marker, so a missed
   ordering is a wait, not a crash; but the Job is what makes the release
   fail when the baseline guard refuses an older/newer database. There is no
   `manage.py migrate` anymore.
3. **RabbitMQ.** Delete the broker (subchart, StatefulSet/Service/PVC,
   `rabbitmq.*` values) and remove `AMQP_URL` and `RABBITMQ_*` from every
   workload. The queue is the `plane_jobs` schema in the same Postgres
   database; no extra service is needed.
4. **Environment.** Remove `CELERY_*`, `GUNICORN_WORKERS` and
   `METRICS_PUSH_INTERVAL_MINUTES`. Keep every other variable and default
   (`apps/server/ENV.md` §4.1). Optional: `PLANE_RS_JOB_CONCURRENCY` (4),
   `PLANE_RS_JOB_LEASE_SECONDS` (60), `PLANE_RS_JOB_DRAIN_SECONDS` (30),
   `PLANE_RS_JOB_POLL_MS` (1000). The worker and beat pods must mount the same
   Secret/ConfigMap as the api pod: `SECRET_KEY` (instance-configuration
   decryption), `DATABASE_URL`, `REDIS_URL`, `AWS_*`, `WEB_URL`, `LIVE_URL`,
   `WEBHOOK_ALLOWED_IPS`, `WEBHOOK_ALLOWED_HOSTS`.
5. **Proxy/Ingress.** Route `/api/*`, `/auth/*` and `/static/*` to the
   `plane-api` Service on port 8000. `/static/*` is now the api's embedded
   copy of the eight files `collectstatic` used to publish, so no static
   volume, no WhiteNoise and no separate static Service are needed. If the
   chart vendors the coexistence Caddyfile (`deploy/caddy/Caddyfile.coexist`,
   `route-groups.yaml`, `PLANE_RS_GROUP_*`), drop all three: they are
   dev/harness-only after cutover.
6. **Probes and logging.** The api answers `GET /` with 200 (Django's
   `{"status": "OK"}`). Use that for readiness/liveness; there is no shell for
   an exec probe. Worker and beat expose no port: use process liveness /
   restart policy. Logs go to stdout; remove the `/code/plane/logs` volumes.
   `INSTANCE_CHANGELOG_URL` is baked into the api image; set it only to
   override.
7. **PVCs/backups.** Remove the RabbitMQ PVC from backup and restore
   procedures. The existing Postgres PVC is enough; `django_celery_beat_*`
   tables stay in the baseline (they are empty and unused).
