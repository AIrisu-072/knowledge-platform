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
 && cp /src/target/release/document-semantic-inspection-worker /out/document-semantic-inspection-worker

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
