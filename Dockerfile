# syntax=docker/dockerfile:1

FROM rust:1.98.1-bookworm AS rust-build
WORKDIR /src

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY tools ./tools
COPY spec ./spec

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --workspace --locked --release \
 && mkdir -p /out \
 && cp /src/target/release/architecture-lint /out/architecture-lint \
 && cp /src/target/release/assure /out/assure

FROM debian:bookworm-slim AS bootstrap-tools
RUN useradd --system --uid 10001 --create-home app
COPY --from=rust-build /out/architecture-lint /usr/local/bin/architecture-lint
COPY --from=rust-build /out/assure /usr/local/bin/assure
USER 10001
ENTRYPOINT ["assure"]
