# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# The exit check's local SMTP sink, webhook listener and fake GitHub. It is a
# throwaway test image, not a deployment component: `sink.rs` is std-only and
# lives outside the Cargo workspace on purpose (`plans/CUTOVER.md` §5).
FROM rust:1.99.1-slim-bookworm AS build
COPY sink.rs /src/sink.rs
RUN rustc -O /src/sink.rs -o /sink \
    && mkdir -p /logs \
    && chown 65532:65532 /logs

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /sink /sink
COPY --from=build --chown=65532:65532 /logs /logs
ENV LOG_DIR=/logs SMTP_PORT=25 HTTP_PORT=8080
EXPOSE 25 8080
ENTRYPOINT ["/sink"]
