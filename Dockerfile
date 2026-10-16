# syntax=docker/dockerfile:1

FROM rust:1.98.1-bookworm AS rust-build
WORKDIR /src

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY tools ./tools
COPY spec ./spec
COPY third_party ./third_party

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --workspace --locked --release \
 && mkdir -p /out \
 && cp /src/target/release/architecture-lint /out/architecture-lint \
 && cp /src/target/release/assure /out/assure \
 && cp /src/target/release/document-publication-scheduler /out/document-publication-scheduler \
 && cp /src/target/release/document-semantic-inspection-worker /out/document-semantic-inspection-worker \
 && cp /src/target/release/document-diff-worker /out/document-diff-worker \
 && cp /src/target/release/document-server /out/document-server \
 && cp /src/target/release/search_outbox_worker /out/search_outbox_worker \
 && cp /src/target/release/search-extraction-worker /out/search-extraction-worker \
 && cp /src/target/release/search-server /out/search-server

# The Document web app's static assets; no Node runtime reaches an image.
FROM node:24.21.0-bookworm-slim AS web-build
WORKDIR /src
RUN corepack enable && corepack prepare pnpm@12.4.1 --activate
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps ./apps
COPY packages ./packages
COPY experiments ./experiments
COPY spec ./spec
RUN pnpm install --frozen-lockfile --filter "@knowledge-platform/document-web..." \
 && pnpm --filter @knowledge-platform/document-web build

# The pinned PDFium of the Document semantic inspection and diff workers.
FROM debian:bookworm-slim AS pdfium
RUN apt-get update \
 && apt-get install -y --no-install-recommends bash ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
COPY experiments/document-semantic-inspection/scripts/install-pdfium.sh /build/experiments/document-semantic-inspection/scripts/install-pdfium.sh
RUN lib_dir="$(bash /build/experiments/document-semantic-inspection/scripts/install-pdfium.sh)" \
 && mkdir -p /opt/pdfium \
 && cp -R "${lib_dir}" /opt/pdfium/lib

# Document server (the PoC fixed identity profile is chosen at run time).
FROM debian:bookworm-slim AS document-server
RUN useradd --system --uid 10001 --create-home app \
 && install -d -o 10001 -g 10001 /var/lib/knowledge-platform/storage
COPY --from=rust-build /out/document-server /usr/local/bin/document-server
COPY --from=rust-build /out/document-semantic-inspection-worker /usr/local/bin/document-semantic-inspection-worker
COPY --from=rust-build /out/document-diff-worker /usr/local/bin/document-diff-worker
COPY --from=web-build /src/apps/document-web/dist /opt/document-web
COPY --from=pdfium /opt/pdfium /opt/pdfium
ENV KP_RUNTIME_MODE=poc \
    KP_DSI_WORKER=/usr/local/bin/document-semantic-inspection-worker \
    KP_DIFF_WORKER=/usr/local/bin/document-diff-worker \
    KP_WEB_DIST=/opt/document-web \
    KP_DSI_PDFIUM_RUNTIME_DIR=/opt/pdfium/lib
USER 10001
ENTRYPOINT ["document-server"]
CMD ["serve"]

# Search outbox worker with the sandboxed extraction worker.
FROM debian:bookworm-slim AS search-worker
RUN useradd --system --uid 10001 --create-home app \
 && install -d -o 10001 -g 10001 /var/lib/knowledge-platform/storage \
    /var/lib/knowledge-platform/search/lexical /var/lib/knowledge-platform/search/config
COPY --from=rust-build /out/search_outbox_worker /usr/local/bin/search_outbox_worker
COPY --from=rust-build /out/search-extraction-worker /usr/local/bin/search-extraction-worker
COPY deploy/compose/search-source.sh /usr/local/bin/search-source.sh
USER 10001
ENTRYPOINT ["search_outbox_worker"]

# Search API server (the PoC fixed identity profile is chosen at run time).
FROM debian:bookworm-slim AS search-server
RUN useradd --system --uid 10001 --create-home app \
 && install -d -o 10001 -g 10001 /var/lib/knowledge-platform/storage \
    /var/lib/knowledge-platform/search/lexical /var/lib/knowledge-platform/search/config
COPY --from=rust-build /out/search-server /usr/local/bin/search-server
ENV KP_RUNTIME_MODE=poc
USER 10001
ENTRYPOINT ["search-server"]
CMD ["serve"]

FROM rust:1.98.1-bookworm AS publication-scheduler
RUN useradd --system --uid 10001 --create-home app
COPY --from=rust-build /out/document-publication-scheduler /usr/local/bin/document-publication-scheduler
COPY --from=rust-build /out/document-semantic-inspection-worker /usr/local/bin/document-semantic-inspection-worker
ENV DSI_WORKER_EXECUTABLE=/usr/local/bin/document-semantic-inspection-worker
USER 10001
ENTRYPOINT ["document-publication-scheduler"]

FROM debian:bookworm-slim AS bootstrap-tools
RUN useradd --system --uid 10001 --create-home app
COPY --from=rust-build /out/architecture-lint /usr/local/bin/architecture-lint
COPY --from=rust-build /out/assure /usr/local/bin/assure
USER 10001
ENTRYPOINT ["assure"]
