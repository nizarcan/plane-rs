ARG PLANE_VERSION=v0.27.1
FROM --platform=$BUILDPLATFORM tonistiigi/binfmt AS binfmt

# **************************************************
#  STAGE 0: Image Loading
# **************************************************
FROM ghcr.io/nizarcan/plane-rs-web:${PLANE_VERSION} AS web-img
# Phase 8 cutover: the backend roles are the Rust images. They are glibc
# binaries, so the runner below is Debian-based rather than the old
# `python:3.12-alpine`; the Python backend image is no longer pulled.
FROM ghcr.io/nizarcan/plane-rs-api:${PLANE_VERSION} AS api-img
FROM ghcr.io/nizarcan/plane-rs-worker:${PLANE_VERSION} AS worker-img
FROM ghcr.io/nizarcan/plane-rs-beat:${PLANE_VERSION} AS beat-img
FROM ghcr.io/nizarcan/plane-rs-migrator:${PLANE_VERSION} AS migrator-img
FROM ghcr.io/nizarcan/plane-rs-space:${PLANE_VERSION} AS space-img
FROM ghcr.io/nizarcan/plane-rs-admin:${PLANE_VERSION} AS admin-img
FROM ghcr.io/nizarcan/plane-rs-live:${PLANE_VERSION} AS live-img
FROM ghcr.io/nizarcan/plane-rs-proxy:${PLANE_VERSION} AS proxy-img

# **************************************************
#  STAGE 1: Runner
# **************************************************
# `node:22-bookworm-slim` gives glibc for the four Rust binaries and the Node
# runtime the space and live servers need. It also ships node/npm; npm is
# removed below because the space program execs its bin shim directly.
FROM node:22-bookworm-slim AS runner

WORKDIR /app

# OS security updates. The Rust binaries need nothing from the image except
# glibc, libgcc and the CA bundle; supervisord comes from apt. Bump
# OS_SECURITY_PATCH to bust buildx's cached layer (keyed on this command
# string), otherwise a rebuild silently re-ships the old packages.
ARG OS_SECURITY_PATCH=2026-11-19
ENV DEBIAN_FRONTEND=noninteractive
RUN echo "aio-security-patch ${OS_SECURITY_PATCH}" \
    && apt-get update \
    && apt-get upgrade --yes \
    && apt-get install --no-install-recommends --yes \
        ca-certificates \
        supervisor \
    && rm -rf /var/lib/apt/lists/* \
    && rm -rf /usr/local/lib/node_modules/npm /usr/local/bin/npm /usr/local/bin/npx

COPY --from=web-img /usr/share/caddy/html /app/web
COPY --from=space-img /app /app/space
COPY --from=admin-img /usr/share/caddy/html/god-mode /app/admin
COPY --from=live-img /app /app/live

RUN rm -rf /app/space/apps/space/.next/cache

COPY --from=proxy-img /usr/bin/caddy /usr/bin/caddy
COPY dist/Caddyfile /app/proxy/Caddyfile

COPY --from=api-img /usr/local/bin/plane-api /usr/local/bin/plane-api
# The published jobs images all use the Dockerfile's default `image` stage,
# whose binary is `/usr/local/bin/plane-jobs` (the role is selected by the
# `JOBS_BINARY` build argument). Rename it back to the path supervisor.conf
# and the compose stacks use.
COPY --from=worker-img /usr/local/bin/plane-jobs /usr/local/bin/plane-worker
COPY --from=beat-img /usr/local/bin/plane-jobs /usr/local/bin/plane-beat
COPY --from=migrator-img /usr/local/bin/plane-jobs /usr/local/bin/plane-migrator

RUN mkdir -p /etc/supervisor/conf.d

COPY start.sh /app/start.sh
COPY dist/plane.env /app/plane.env
COPY supervisor.conf /etc/supervisor/conf.d/supervisor.conf

RUN mkdir -p /app/logs/access && \
    mkdir -p /app/logs/error && \
    mkdir -p /app/data && \
    chmod +x /app/start.sh

VOLUME ["/app/data", "/app/logs"]

EXPOSE 80 443

CMD ["/app/start.sh"]
