# Images are pinned by digest (Renovate bumps tag and digest together): a re-pushed tag cannot
# change what gets built.
# Builder, runtime and development.Dockerfile share one Debian release (13 "trixie"), so the
# binary links against the glibc and OpenSSL the runtime image ships (MAIR-741: Debian 12's
# distroless libssl3 is unpatched).
FROM rust:1.99-slim-trixie@sha256:b8e062b1c3a80183df83d9ba187cadd2b5ab84bc78d453dbfeb2fccd5f982247 AS builder

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
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1
WORKDIR /app
COPY --from=builder /usr/src/app/target/release/compliance_api /app/compliance-api
# Backup sealing tool (MAIR-500), run by the backup Job of Devops/Deploiment.
COPY --from=builder /usr/src/app/target/release/compliance_backup /app/compliance-backup

USER 65532:65532
CMD ["/app/compliance-api"]
