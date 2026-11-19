# plane-rs Helm chart

Plane Community Edition with the **Rust backend** (`plane-rs`): `plane-api`,
`plane-worker`, `plane-beat` and `plane-migrator`, plus the `web`, `space`,
`admin` and `live` frontends. There is no Python process, no Celery and no
RabbitMQ in the stack: the job queue is the `plane_jobs` schema in the same
PostgreSQL database.

The chart is the Kubernetes equivalent of `docker-compose.yml` and
`deployments/cli/community/docker-compose.yml` in this repository, and it
follows `deployments/kubernetes/community/README.md` (the Phase 8 cutover
notes) and `apps/server/DECISIONS.md` D34/D36.

## What it deploys

| Resource | Port | Notes |
| --- | --- | --- |
| Deployment `…-api` | 8000 | `plane-api`; serves `/api`, `/auth` and the embedded `/static` files |
| Deployment `…-worker` | — | `plane_jobs` worker; no HTTP listener |
| Deployment `…-beat` | — | scheduler; **exactly one replica** |
| Job `…-migrator-<8 hex>` | — | Django baseline + `plane_jobs` schema |
| Deployment `…-web` | 3000 | static web app (Caddy) |
| Deployment `…-space` | 3000 | public project pages (`/spaces`) |
| Deployment `…-admin` | 3000 | god mode (`/god-mode`) |
| Deployment `…-live` | 3000 | Hocuspocus WebSocket server (`/live`) |
| StatefulSet `…-postgresql` | 5432 | optional, `database.bundled` |
| StatefulSet `…-redis` | 6379 | Valkey, `redis.bundled` |
| StatefulSet `…-minio` | 9000 | pgsty/silo, `minio.bundled` |
| Job `…-minio-bucket-<8 hex>` | — | post-install hook, creates the bucket when MinIO is bundled |
| Ingress | — | optional, mirrors `apps/proxy/Caddyfile.ce` |
| Services | 8000/3000 | always created (ClusterIP) |

Images are `ghcr.io/nizarcan/plane-rs-{api,worker,beat,migrator,web,space,admin,live}`
and default to `.Chart.AppVersion` (the pinned upstream version this rewrite
tracks). Override `images` per component or set `fullnameOverride`.

The four Rust images are distroless (`gcr.io/distroless/cc-debian12:nonroot`,
UID 65532, no shell). The frontend images are Caddy/Node images and run as
root; their security contexts are empty in `values.yaml` and can be tightened
per component.

## Requirements

- Kubernetes 1.27+ (Job `ttlSecondsAfterFinished`, `seccompProfile`)
- Helm 3.14+
- Images: `linux/amd64` and `linux/arm64` (the release workflow builds both;
  the default footprint targets a small arm64 cluster)

## Install

```bash
helm install plane-rs charts/plane-rs \
  --namespace plane --create-namespace \
  --set secrets.app.secretKey="$(openssl rand -hex 32)" \
  --set secrets.app.liveServerSecretKey="$(openssl rand -hex 32)" \
  --set ingress.enabled=true \
  --set ingress.className=nginx \
  --set ingress.host=plane.example.com
```

`SECRET_KEY` is not a rotation candidate: it derives the key that encrypts the
instance-configuration rows (SMTP password, OAuth client secrets). Changing it
later makes those rows undecryptable **without an error**. Set it once, keep it
in a Secret, and set `secrets.app.requireExplicitSecrets=true` so a render with
the placeholder fails.

Without ingress the stack is still reachable:

```bash
kubectl -n plane port-forward svc/plane-rs-web 3000:3000
kubectl -n plane port-forward svc/plane-rs-api 8000:8000
```

The API health endpoint is `GET /` (Django's `{"status": "OK"}`); it does not
touch Postgres or Redis, which is why it is used for all three probes. A
port-forward alone gives you the web assets but not a working UI, because the
frontends call `/api` on their own origin; use ingress (or any reverse proxy)
for a usable installation.

### Ingress

The Ingress mirrors `Caddyfile.ce`:

| Path | Service |
| --- | --- |
| `/api`, `/auth`, `/static` | `…-api:8000` |
| `/spaces` | `…-space:3000` |
| `/god-mode` | `…-admin:3000` |
| `/live` | `…-live:3000` (WebSocket) |
| `/uploads` | `…-minio:9000` (only when MinIO is bundled) |
| `/` | `…-web:3000` |

Recommended annotations for nginx:

```yaml
ingress:
  annotations:
    nginx.ingress.kubernetes.io/proxy-body-size: 5m          # = config.fileSizeLimit
    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"   # /live
    nginx.ingress.kubernetes.io/proxy-send-timeout: "3600"
```

Each `ingress.paths.*` can be overridden; an empty value disables that route.
The bundled Caddy redirects `/spaces` and `/god-mode` to their trailing-slash
form; an Ingress does not, and the frontends serve both forms themselves.

## Upgrade

```bash
helm upgrade plane-rs charts/plane-rs -n plane --reuse-values \
  --set api.image.tag=v1.5.0-rc2 ...
```

Every workload carries a `checksum/config` (and, for chart-created Secrets, a
`checksum/secret-*`) annotation, so a values/template/Secret change rolls the
pods; a new image **under the same tag** does not, exactly as with any chart.
Use a new tag per release (the release workflow tags `v*`).

The migrator Job is named `<fullname>-migrator-<8 hex>`, a checksum of the
things that must make it run again: the migrator image reference, the rendered
ConfigMap and database Secret (or the existing database Secret's name), whether
the database is bundled, and the rest of its pod template. An identical render
keeps the same Job (apply is a no-op and a completed Job stays completed); a
changed input creates a new Job, so a Job's immutable pod template is never
patched. The name deliberately does **not** use `.Release.Revision`, which
ArgoCD's `helm template` always renders as `1` (see the ArgoCD section).

- `migrator.hookMode: auto` (default) renders it as a `pre-install,pre-upgrade`
  hook (ArgoCD maps both to `PreSync`) **only when the database is external and
  `secrets.database.existingSecret` is set** — the only configuration where
  the Secret and the database exist before Helm creates the release. In every
  other case (in particular with the bundled PostgreSQL) it is a regular Job:
  a pre-install hook runs *before* the chart's StatefulSets and Secrets exist,
  so there is nothing to connect to. The api/worker/beat binaries then block in
  `wait_for_migrations` until the Job has written its marker, which makes a
  missed ordering a wait, not a crash.
- With the bundled PostgreSQL the Job runs a `wait-for-postgresql` init
  container (`pg_isready`) before the migrator, so a cold database does not
  consume `backoffLimit` attempts — under ArgoCD the Job and the StatefulSet
  are applied in the same sync wave.
- A baseline guard refusal (database older or newer than the pinned Django
  migration set) exits non-zero and the release fails loudly. That is the
  intended behaviour: run the *previous* release's `plane-migrator` against a
  database you are upgrading from Django, or restore a dump first.
- `backoffLimit: 3`, `activeDeadlineSeconds: 900`, and
  `ttlSecondsAfterFinished: 86400` (logs stay for a day).

### ArgoCD

ArgoCD renders charts with `helm template`, so `.Release.Revision` is always
`1` and `.Release.IsUpgrade` is always false. The chart uses neither: the
migrator and bucket Job names are content-addressed, and Helm hook annotations
are what ArgoCD translates into sync hooks (`pre-install,pre-upgrade` →
`PreSync`, `post-install,post-upgrade` → `PostSync`, `before-hook-creation` →
`BeforeHookCreation`).

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: plane
  namespace: argocd
spec:
  project: default
  source:
    repoURL: https://github.com/nizarcan/plane-rs
    targetRevision: main          # or a v* release tag
    path: charts/plane-rs
    helm:
      # Inline values, or `valueFiles` pointing at a file committed inside the
      # repository (ArgoCD refuses paths outside it); secrets belong in ESO.
      parameters:
        - name: secrets.app.existingSecret
          value: plane-app-secrets
        - name: secrets.database.existingSecret
          value: plane-database-secrets
        - name: secrets.objectStorage.existingSecret
          value: plane-object-storage-secrets
        - name: database.bundled
          value: "false"
        - name: ingress.enabled
          value: "true"
        - name: ingress.host
          value: plane.example.com
  destination:
    server: https://kubernetes.default.svc
    namespace: plane
  syncPolicy:
    automated:
      prune: true
      selfHeal: true
    syncOptions:
      - CreateNamespace=true
```

ArgoCD names the Helm release after the Application (`plane` here), and the
chart's fullname helper uses the release name when it contains the chart name,
so the resources are `plane-api`, `plane-worker`, `plane-config`, ….

Two migrator shapes under ArgoCD:

- **External database + `secrets.database.existingSecret` (recommended for
  ArgoCD).** `hookMode: auto` emits `helm.sh/hook: pre-install,pre-upgrade`,
  which ArgoCD runs as **PreSync**. The schema step finishes (or fails the
  sync) before any workload is applied.
- **Bundled PostgreSQL, or chart-created Secrets.** The Job is a normal
  resource. It is applied in the same sync wave as the StatefulSet and the
  Secrets, its `wait-for-postgresql` init container holds it until the database
  is up, and `wait_for_migrations` holds api/worker/beat until the marker
  exists. No `Sync` hook annotation is used on purpose: ArgoCD re-runs Sync
  hooks on **every** sync, so an unchanged render would delete and re-create the
  Job and rerun the migration each time; as a normal resource, an unchanged
  render keeps the completed Job and the Application stays Healthy, while a
  failed Job shows as Degraded and a changed input produces a new Job that the
  next sync creates (the old one is pruned, or removed by
  `ttlSecondsAfterFinished`).

Because the Job's health participates in the Application's health, a baseline
refusal leaves the Application `Degraded` with the migrator log available; fix
the database and sync again (the Job name is unchanged if the inputs are, so
delete the failed Job or change a value to force a new one — a failed Job with
the same name is not re-run by `kubectl apply`).

### Upgrading from a Django-migrated database

Point the chart at the existing database and let the migrator verify the
baseline:

```yaml
database:
  bundled: false
  url: postgresql://plane:…@postgres.example.com:5432/plane
# or leave `url` empty and set secrets.database.existingSecret
```

`plane-migrator` compares `django_migrations` with the embedded baseline:
equal → `baseline matched` and the `plane_jobs` schema is created if missing;
fewer rows → it refuses and names `apps/api/bin/docker-entrypoint-migrator.sh`;
more rows → it refuses as a downgrade; a non-empty `public` without
`django_migrations` → it refuses. The Django tables stay in place; nothing is
dropped. `django_celery_beat_*` tables stay in the baseline (empty and unused).

## External services

```yaml
database:
  bundled: false
  url: postgresql://plane:plane@postgres.example.com:5432/plane?sslmode=require
redis:
  bundled: false
  url: rediss://:password@redis.example.com:6379/0
minio:
  bundled: false
objectStorage:
  endpoint: https://s3.eu-central-1.amazonaws.com
  region: eu-central-1
  bucket: plane-uploads
```

`REDIS_URL` is rendered into the shared ConfigMap, so a URL with a password is
visible in the ConfigMap. Put it in the app Secret instead (a Secret key
overrides the ConfigMap key of the same name):

```yaml
secrets:
  app:
    existingSecret: plane-app-secrets   # contains SECRET_KEY, LIVE_SERVER_SECRET_KEY, REDIS_URL
```

With `minio.bundled=false`, `USE_MINIO` is `0` and the browser gets presigned
URLs pointing at `objectStorage.endpoint`. An **external MinIO** needs the
custom-domain behaviour instead (`USE_MINIO=1`, `https://<host>/<bucket>/…`
URLs): set `config.extra.USE_MINIO: "1"` and route `/<bucket>` to that MinIO
with your own Ingress; `config.extra` wins over every computed key.

## Secrets and External Secrets Operator

Three secret groups, each either created from values or taken from an existing
Secret:

| Group | `existingSecret` | Keys |
| --- | --- | --- |
| app | `secrets.app.existingSecret` | `SECRET_KEY`, `LIVE_SERVER_SECRET_KEY` (plus any ConfigMap override, e.g. `REDIS_URL`) |
| database | `secrets.database.existingSecret` | `DATABASE_URL`; `POSTGRES_PASSWORD` for the bundled PostgreSQL |
| object storage | `secrets.objectStorage.existingSecret` | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` (also MinIO's root credentials) |

The chart never reads an existing Secret; it only references it by name, so
ESO, Sealed Secrets or a hand-made Secret all work. With ESO, let the chart
create everything else and render the operator objects with `extraObjects`:

```yaml
secrets:
  app:
    existingSecret: plane-app-secrets
  database:
    existingSecret: plane-database-secrets
  objectStorage:
    existingSecret: plane-object-storage-secrets

extraObjects:
  - apiVersion: external-secrets.io/v1beta1
    kind: SecretStore
    metadata:
      name: plane-vault
    spec:
      provider:
        vault:
          server: https://vault.example.com
          path: kv/plane
          auth:
            kubernetes:
              mountPath: kubernetes
              role: plane
  - apiVersion: external-secrets.io/v1beta1
    kind: ExternalSecret
    metadata:
      name: plane-app-secrets
    spec:
      refreshInterval: 1h
      secretStoreRef:
        name: plane-vault
        kind: SecretStore
      target:
        name: plane-app-secrets
        creationPolicy: Owner
      data:
        - secretKey: SECRET_KEY
          remoteRef: { key: plane/app, property: secret_key }
        - secretKey: LIVE_SERVER_SECRET_KEY
          remoteRef: { key: plane/app, property: live_server_secret_key }
```

`external-secrets.io` CRDs are not part of the chart's validation set; the
objects are passed through untemplated, so ESO's own `{{ }}` placeholders
survive.

## Probes, resources and the one-beat rule

| Component | Readiness / liveness | Why |
| --- | --- | --- |
| api | `GET /` (startup probe allows 5 min for the boot steps) | health JSON, no DB/Redis access |
| web | `GET /` | SPA fallback always answers |
| space | `GET /spaces/` | same path as the image's own HEALTHCHECK |
| admin | `GET /god-mode/` | trailing slash: `/god-mode` is a 301 |
| live | TCP 3000 | plain GETs 404; a TCP probe still detects a dead listener |
| worker, beat | none | no HTTP listener and a distroless image with no shell, so there is nothing to GET or exec; a lost DB/Redis connection exits non-zero and the pod restarts |
| migrator | none | a one-shot binary reports success by exiting 0 |

Default resources follow `apps/server/BENCH.md`: `plane-api` idles at ~18–60 MiB
(requests 64Mi, limit 256Mi), worker/beat at a few MiB (32Mi/24Mi requests).
Frontends request 32–64Mi. PostgreSQL requests 128Mi. Everything fits a 2–4 GiB
arm64 node; raise the limits if builds or exports run on the worker.

`beat.replicas` must be `1`: the scheduler owns the schedule rows in
`plane_jobs`, and a second replica would fire every due job twice. The chart
fails the render on any other value and uses the `Recreate` strategy.

## Values

| Key | Default | Description |
| --- | --- | --- |
| `nameOverride` / `fullnameOverride` | `""` | Resource-name overrides |
| `imagePullSecrets` | `[]` | Pull secrets for a private GHCR package |
| `serviceAccount.create` | `true` | One ServiceAccount for every pod |
| `serviceAccount.annotations` | `{}` | e.g. `eks.amazonaws.com/role-arn` |
| `api.replicas` | `1` | Stateless; scale freely |
| `api.image.repository` | `ghcr.io/nizarcan/plane-rs-api` | |
| `{api,worker,beat,migrator,web,space,admin,live}.image.tag` | `""` | Defaults to `.Chart.AppVersion` |
| `{component}.resources` | see `values.yaml` | Requests/limits per component |
| `{component}.podSecurityContext` / `.containerSecurityContext` | Rust: nonroot 65532; frontends: `{}` | |
| `{component}.nodeSelector` / `.tolerations` / `.affinity` | `{}` / `[]` / `{}` | |
| `api/worker/beat/migrator.extraEnv` | `[]` | Extra `env:` entries |
| `migrator.hookMode` | `auto` | `auto`, `always`, `never` (see Upgrade); the Job is content-addressed `<fullname>-migrator-<8 hex>` |
| `migrator.backoffLimit` | `3` | |
| `migrator.activeDeadlineSeconds` | `900` | |
| `config.webUrl` | `""` | Derived from ingress, else `http://localhost` |
| `config.corsAllowedOrigins` | `""` | Empty = allow all origins (upstream default) |
| `config.allowedHosts` | `*` | |
| `config.fileSizeLimit` | `5242880` | Match the ingress body size |
| `config.numProxies` | `""` | 1 behind the ingress |
| `config.webhookAllowedIps` / `.webhookAllowedHosts` | `""` | Webhook SSRF allowlists |
| `config.logFormat` | `json` | `json`, `pretty`, `compact` |
| `config.dbPoolSize` | `16` | `PLANE_RS_DB_POOL_SIZE` |
| `config.jobConcurrency` | `4` | `PLANE_RS_JOB_CONCURRENCY` |
| `config.jobLeaseSeconds` | `60` | `PLANE_RS_JOB_LEASE_SECONDS` |
| `config.jobDrainSeconds` | `30` | `PLANE_RS_JOB_DRAIN_SECONDS` |
| `config.jobPollMs` / `config.beatPollMs` | `1000` | LISTEN/NOTIFY fallbacks |
| `config.extra` | `{}` | Extra ConfigMap keys; wins over computed values |
| `secrets.app.secretKey` | placeholder | **Set this** (see Install) |
| `secrets.app.liveServerSecretKey` | placeholder | **Set this** |
| `secrets.app.requireExplicitSecrets` | `false` | Fail on the placeholder |
| `secrets.database.password` | `plane` | Bundled PostgreSQL superuser |
| `secrets.objectStorage.accessKey` / `.secretKey` | `access-key` / `secret-key` | |
| `database.bundled` | `true` | |
| `database.url` | `""` | External `DATABASE_URL` |
| `database.persistence.storageClass` / `.size` | `""` / `8Gi` | |
| `database.persistence.existingClaim` | `""` | Use a pre-provisioned PVC |
| `redis.bundled` / `redis.url` | `true` / `""` | |
| `redis.persistence.storageClass` / `.size` | `""` / `1Gi` | |
| `minio.bundled` / `minio.createBucket` | `true` / `true` | |
| `minio.persistence.storageClass` / `.size` | `""` / `8Gi` | |
| `objectStorage.bucket` / `.region` / `.endpoint` | `uploads` / `""` / `""` | |
| `objectStorage.minioEndpointSsl` | `false` | `MINIO_ENDPOINT_SSL=1` |
| `live.apiBaseUrl` | `""` | Defaults to `http://<fullname>-api:8000` |
| `ingress.enabled` / `.className` / `.host` | `false` / `""` / `plane.example.com` | |
| `ingress.tls.enabled` / `.secretName` | `false` / `""` | Empty name lets cert-manager create it |
| `ingress.paths` | see above | Empty value disables a route |
| `extraObjects` | `[]` | Passed through verbatim |

`APP_VERSION`, which `/api/instances/` reports and `register_instance` writes,
is set to the api image tag (`.Chart.AppVersion` unless
`api.image.tag` overrides it); the Django image read it from `package.json`,
which a distroless image does not have.

## Any other environment variable

Every setting in `apps/server/ENV.md` can be passed through:

- `config.extra` adds plain values to the shared ConfigMap (api, worker, beat,
  migrator), e.g. SMTP or signup switches;
- `<component>.extraEnv` adds `env:` entries (with `valueFrom`, for values that
  live in a Secret) to one component;
- a Secret named in `secrets.app.existingSecret` can carry any key and overrides
  the ConfigMap key of the same name for api, worker, beat and migrator.

```yaml
config:
  extra:
    EMAIL_HOST: smtp.example.com
    EMAIL_PORT: "587"
    EMAIL_FROM: Plane <plane@example.com>
    EMAIL_USE_TLS: "1"
    ENABLE_SIGNUP: "0"
api:
  extraEnv:
    - name: EMAIL_HOST_PASSWORD
      valueFrom:
        secretKeyRef:
          name: plane-app-secrets
          key: EMAIL_HOST_PASSWORD
worker:
  extraEnv:
    - name: EMAIL_HOST_PASSWORD
      valueFrom:
        secretKeyRef:
          name: plane-app-secrets
          key: EMAIL_HOST_PASSWORD
```

`EMAIL_HOST_PASSWORD`, the OAuth client secrets and `LLM_API_KEY` are seeded
into `instance_configurations` on the first api boot and encrypted with
`SECRET_KEY`; after that the database row wins over the environment
(`SKIP_ENV_VAR=1`), so change them through the admin API, not by editing
values.

`PLANE_RS_JOBS_RUST_TASKS`, `PLANE_RS_BOOT_STEPS` and the `PLANE_RS_JOBS_*`
handover knobs are test-harness settings; do not set them in a deployment.

## Known limits

- The frontend images bake `VITE_*` base URLs at build time. Serving the apps
  from separate hostnames (instead of one host with path prefixes) needs images
  built with those values; the chart's ingress is path-based by design.
- `plane-api`'s boot steps (`register_instance`, `configure_instance`,
  `create_bucket`, `clear_cache`) run once per pod start. `clear_cache` is a
  Redis `FLUSHDB`, so starting a second api replica also clears the cache
  (upstream behaviour, `apps/server/DECISIONS.md` D34). `create_bucket` never
  fails the boot, so the chart adds the bucket hook when MinIO is bundled; an
  external S3 bucket must exist (or be creatable by the credentials).
- There is no `collectstatic`, no static volume and no WhiteNoise: `/static/` is
  served from the api binary.
- HPA/PDB are not included. The api is stateless and can be scaled; the worker
  scales with `PLANE_RS_JOB_CONCURRENCY` (lease-based queue), never the beat.
- `volumeClaimTemplates` of the bundled StatefulSets are immutable, like every
  StatefulSet: changing `persistence.size` or `storageClass` later needs the
  claims resized/expanded out of band (or an `existingClaim`).
- A failed migrator Job is not re-run by an unchanged render (its name is the
  checksum of unchanged inputs). Delete the Job, or change an input, to retry
  after fixing the database. This is the same under Helm and ArgoCD and is the
  price of "an unchanged render keeps the same Job".
