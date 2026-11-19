{{/*
Copyright (c) 2023-present Plane Software, Inc. and contributors
SPDX-License-Identifier: AGPL-3.0-only
See the LICENSE file for details.
*/}}
{{/*
Name helpers.
*/}}
{{- define "plane-rs.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "plane-rs.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{/* plane-rs.componentName (dict "ctx" $ "component" "api") */}}
{{- define "plane-rs.componentName" -}}
{{- printf "%s-%s" (include "plane-rs.fullname" .ctx) .component | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Labels. Everything is namespaced by the release so two releases in one
namespace never select each other's pods.
*/}}
{{- define "plane-rs.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .ctx.Chart.Name .ctx.Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
app.kubernetes.io/name: {{ include "plane-rs.name" .ctx }}
app.kubernetes.io/instance: {{ .ctx.Release.Name }}
app.kubernetes.io/version: {{ .ctx.Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .ctx.Release.Service }}
app.kubernetes.io/part-of: {{ include "plane-rs.name" .ctx }}
{{- with .component }}
app.kubernetes.io/component: {{ . }}
{{- end }}
{{- end -}}

{{- define "plane-rs.selectorLabels" -}}
app.kubernetes.io/name: {{ include "plane-rs.name" .ctx }}
app.kubernetes.io/instance: {{ .ctx.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end -}}

{{/*
Images. Every image is `<repository>:<tag>`; the tag defaults to the chart's
appVersion so a release bump moves every component at once.
*/}}
{{- define "plane-rs.image" -}}
{{- printf "%s:%s" .image.repository (default .ctx.Chart.AppVersion .image.tag) -}}
{{- end -}}

{{- define "plane-rs.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "plane-rs.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/*
Secret and ConfigMap names. An `existingSecret` always wins.
*/}}
{{- define "plane-rs.configMapName" -}}
{{- printf "%s-config" (include "plane-rs.fullname" .) -}}
{{- end -}}

{{- define "plane-rs.appSecretName" -}}
{{- if .Values.secrets.app.existingSecret -}}
{{- .Values.secrets.app.existingSecret -}}
{{- else -}}
{{- printf "%s-app" (include "plane-rs.fullname" .) -}}
{{- end -}}
{{- end -}}

{{- define "plane-rs.databaseSecretName" -}}
{{- if .Values.secrets.database.existingSecret -}}
{{- .Values.secrets.database.existingSecret -}}
{{- else -}}
{{- printf "%s-database" (include "plane-rs.fullname" .) -}}
{{- end -}}
{{- end -}}

{{- define "plane-rs.objectStorageSecretName" -}}
{{- if .Values.secrets.objectStorage.existingSecret -}}
{{- .Values.secrets.objectStorage.existingSecret -}}
{{- else -}}
{{- printf "%s-object-storage" (include "plane-rs.fullname" .) -}}
{{- end -}}
{{- end -}}

{{/*
Environment sources. The ConfigMap first, then the Secrets: a Secret key
overrides the ConfigMap key of the same name, which is the documented override
path for e.g. a REDIS_URL that carries a password.

The migrator only needs the database (and the ConfigMap, which it ignores): it
must not depend on the app or object-storage Secrets so a pre-install hook has
the smallest possible set of prerequisites.
*/}}
{{- define "plane-rs.backendEnvFrom" -}}
- configMapRef:
    name: {{ include "plane-rs.configMapName" . }}
- secretRef:
    name: {{ include "plane-rs.appSecretName" . }}
- secretRef:
    name: {{ include "plane-rs.databaseSecretName" . }}
- secretRef:
    name: {{ include "plane-rs.objectStorageSecretName" . }}
{{- end -}}

{{- define "plane-rs.migratorEnvFrom" -}}
- configMapRef:
    name: {{ include "plane-rs.configMapName" . }}
- secretRef:
    name: {{ include "plane-rs.databaseSecretName" . }}
{{- end -}}

{{/*
Computed configuration. Every function here is used both by the ConfigMap/Secret
templates and by the NOTES/README examples, so keep them side-effect free.
*/}}
{{- define "plane-rs.webUrl" -}}
{{- if .Values.config.webUrl -}}
{{- .Values.config.webUrl -}}
{{- else if .Values.ingress.enabled -}}
{{- printf "%s://%s" (ternary "https" "http" .Values.ingress.tls.enabled) .Values.ingress.host -}}
{{- else -}}
http://localhost
{{- end -}}
{{- end -}}

{{/* The URL used to build the DATABASE_URL when the chart owns the Secret. */}}
{{- define "plane-rs.databaseUrl" -}}
{{- if .Values.database.url -}}
{{- .Values.database.url -}}
{{- else if .Values.database.bundled -}}
{{- printf "postgresql://%s:%s@%s:%v/%s" .Values.database.user .Values.secrets.database.password (include "plane-rs.componentName" (dict "ctx" . "component" "postgresql")) (.Values.database.port | int) .Values.database.name -}}
{{- else -}}
{{- fail "database.bundled=false needs database.url (or set secrets.database.existingSecret)" -}}
{{- end -}}
{{- end -}}

{{- define "plane-rs.redisUrl" -}}
{{- if .Values.redis.bundled -}}
{{- printf "redis://%s:%v/0" (include "plane-rs.componentName" (dict "ctx" . "component" "redis")) (.Values.redis.port | int) -}}
{{- else -}}
{{- required "redis.bundled=false needs redis.url (or an app Secret with a REDIS_URL key)" .Values.redis.url -}}
{{- end -}}
{{- end -}}

{{- define "plane-rs.objectStorageEndpoint" -}}
{{- if .Values.minio.bundled -}}
{{- printf "http://%s:%v" (include "plane-rs.componentName" (dict "ctx" . "component" "minio")) (.Values.minio.port | int) -}}
{{- else -}}
{{- required "minio.bundled=false needs objectStorage.endpoint" .Values.objectStorage.endpoint -}}
{{- end -}}
{{- end -}}

{{/* The bundled MinIO is always addressed as a MinIO (custom domain), never as
an S3 API host, because the ingress routes /<bucket>/ to it. */}}
{{- define "plane-rs.useMinio" -}}
{{- if .Values.minio.bundled -}}
1
{{- else -}}
0
{{- end -}}
{{- end -}}

{{/*
Content-addressed identity for the migrator Job.

ArgoCD renders with `helm template`, where `.Release.Revision` is always 1, so a
revision-suffixed name never changes there -- and a Job's pod template is
immutable, so the first image bump would fail to sync. The name is therefore
derived from everything that must make the Job run again:

  * the migrator image reference (repository + resolved tag + pull policy);
  * the rendered shared ConfigMap and the rendered database Secret (i.e. the
    DATABASE_URL), or the *name* of an existing database Secret;
  * whether the database is bundled (the wait-for-postgresql init container);
  * every other input to the pod template (env, envFrom, resources, security
    contexts, scheduling, labels, annotations, service account, pull secrets).

An identical render keeps the name (Helm/ArgoCD apply is a no-op, the completed
Job stays completed); any change to those inputs creates a new Job, so an
immutable field is never patched and the migration reruns. A change to another
component's values (api replicas, ingress, frontends) does not change the name.

The checksum is not a security boundary; it only has to change when the inputs
do. `toYaml` marshals map keys sorted, so the digest is deterministic.
*/}}
{{- define "plane-rs.migratorChecksum" -}}
{{- $inputs := dict
  "image" (include "plane-rs.image" (dict "ctx" . "image" .Values.migrator.image))
  "pullPolicy" .Values.migrator.image.pullPolicy
  "configMap" (include (print .Template.BasePath "/configmap.yaml") .)
  "databaseSecret" (include (print .Template.BasePath "/secret-database.yaml") .)
  "databaseExistingSecret" .Values.secrets.database.existingSecret
  "databaseBundled" (toString .Values.database.bundled)
  "databaseUser" .Values.database.user
  "databaseName" .Values.database.name
  "databasePort" (toString .Values.database.port)
  "extraEnv" (toYaml .Values.migrator.extraEnv)
  "extraEnvFrom" (toYaml .Values.migrator.extraEnvFrom)
  "podSecurityContext" (toYaml .Values.migrator.podSecurityContext)
  "containerSecurityContext" (toYaml .Values.migrator.containerSecurityContext)
  "resources" (toYaml .Values.migrator.resources)
  "nodeSelector" (toYaml .Values.migrator.nodeSelector)
  "tolerations" (toYaml .Values.migrator.tolerations)
  "affinity" (toYaml .Values.migrator.affinity)
  "podLabels" (toYaml .Values.migrator.podLabels)
  "podAnnotations" (toYaml .Values.migrator.podAnnotations)
  "serviceAccount" (include "plane-rs.serviceAccountName" .)
  "imagePullSecrets" (toYaml .Values.imagePullSecrets)
-}}
{{- $inputs | toYaml | sha256sum | trunc 8 -}}
{{- end -}}

{{/* `<fullname>-migrator-<8 hex>`. The fullname is shortened (43 + 1 + 8 +
1 + 8 = 61 characters) rather than the finished name, so the component suffix
and the digest always survive the 63-character DNS-label limit. */}}
{{- define "plane-rs.migratorJobName" -}}
{{- $full := include "plane-rs.fullname" . | trunc 43 | trimSuffix "-" -}}
{{- printf "%s-migrator-%s" $full (include "plane-rs.migratorChecksum" .) -}}
{{- end -}}

{{/* A deterministic 8-hex digest of an arbitrary dict of inputs. Used to name
immutable Jobs from their content, so `helm template` (and therefore ArgoCD,
where `.Release.Revision` is always 1) produces a new name exactly when the
inputs change. Included with a dict: `include "plane-rs.hash8" (dict ...)`. */}}
{{- define "plane-rs.hash8" -}}
{{- toYaml . | sha256sum | trunc 8 -}}
{{- end -}}

{{/* Scheduling stanza, included at the pod-spec level:
     {{- include "plane-rs.podScheduling" .Values.api | nindent 6 }} */}}
{{- define "plane-rs.podScheduling" -}}
{{- with .nodeSelector }}
nodeSelector:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- with .tolerations }}
tolerations:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- with .affinity }}
affinity:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- end -}}

{{/* Deployment strategy: never take an old api/web pod out before a new one is
ready, so a rolling release is seamless on a single-replica install. */}}
{{- define "plane-rs.strategy" -}}
strategy:
  type: RollingUpdate
  rollingUpdate:
    maxUnavailable: 0
    maxSurge: 1
{{- end -}}
