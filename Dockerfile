# syntax=docker/dockerfile:1.7

FROM node:24-bookworm-slim AS web-builder
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.99-bookworm AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked && cp target/release/thunder-remoted /thunder-remoted

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust-builder /thunder-remoted /usr/local/bin/thunder-remoted
COPY --from=web-builder /web/dist ./web/dist
ENV THUNDER_BIND=0.0.0.0:3600 THUNDER_NAS_URL=http://127.0.0.1:2345
EXPOSE 3600
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/thunder-remoted"]
CMD ["serve"]
