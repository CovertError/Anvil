# Dockerfile for `anvil-bench` — the in-process HTTP throughput tester.
#
# Build:
#   docker build -f tools/http-bench/Dockerfile -t anvilforge-bench .
#
# Run (in-process loopback against sqlite::memory: — the existing default):
#   docker run --rm anvilforge-bench -- -c 200 -s 10
#
# Run with Postgres in the loop (use the docker-compose.yml next to this file):
#   docker compose -f tools/http-bench/docker-compose.yml up --build --abort-on-container-exit bench

# ── builder ───────────────────────────────────────────────────────────────
FROM rust:1.85-slim AS builder

WORKDIR /src

# Apt minimums for the openssl/sqlx-postgres feature set we already pull in.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Copy the workspace. We don't pre-fetch deps because the dockerfile is
# expected to run end-to-end in CI — pre-fetch tricks like cargo-chef are
# the right call once we want to optimize CI cache hit rate.
COPY . .

RUN cargo build --release -p anvilforge-http-bench

# ── runtime ───────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/anvil-bench /usr/local/bin/anvil-bench

ENTRYPOINT ["anvil-bench"]
CMD ["--help"]
