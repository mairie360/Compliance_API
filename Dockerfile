# Images are pinned by digest (Renovate bumps tag and digest together): a re-pushed tag cannot
# change what gets built.
FROM rust:1.99-slim-bookworm@sha256:2c3a22f0a5533ea2dd5a16627bc841228151faa2d4de2644ac9987e4a2f1f2fa AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app

# --- DEPENDENCY CACHE ---
# Build the dependencies alone first: this layer stays cached as long as Cargo.toml and Cargo.lock
# do not change, so a code change only recompiles the API itself.
# `--locked`: build exactly the reviewed `Cargo.lock`, fail instead of resolving new versions.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src
# -----------------------------

COPY . .
# Make sure cargo rebuilds the crate instead of keeping the placeholder built above.
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

# --- Stage 2: runtime (distroless, non-root) ---
# The `nonroot` variant runs as uid/gid 65532. `USER` is repeated numerically so
# Kubernetes can enforce `runAsNonRoot: true`. The API binds an unprivileged
# port (`PORT`, 3000+) and never writes to the filesystem. The binary stays
# owned by root (read + execute only for the runtime user).
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
WORKDIR /app
COPY --from=builder /usr/src/app/target/release/compliance_api /app/compliance-api

USER 65532:65532
CMD ["/app/compliance-api"]
