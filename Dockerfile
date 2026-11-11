# syntax=docker/dockerfile:1.7

FROM rust:1.99-bookworm AS builder

WORKDIR /src
ARG THUNDER_TASK_RS_REV=main
ENV RUSTUP_TOOLCHAIN=1.99.0

COPY . .
# The library is pulled from the lib-x GitHub repository at build time;
# pin a revision for reproducible builds via THUNDER_TASK_RS_REV.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked || cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/thunder-remoted /usr/local/bin/thunder-remoted

ENV THUNDER_SESSION_DIR=/data
WORKDIR /data
VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/thunder-remoted"]
