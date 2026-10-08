# Same toolchain as the production `Dockerfile`, pinned by digest.
FROM rust:1.99-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS development

RUN apt update && apt install -y curl && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-watch --locked

# Run as an unprivileged user: it owns the sources, `target/` and the cargo registry cache so
# `cargo watch` can rebuild and Compose `develop.watch` can sync files into the container.
# change api name
RUN useradd --create-home --uid 1000 dev \
    && mkdir -p /usr/src/template \
    && chown -R dev:dev /usr/src/template /usr/local/cargo
COPY --chmod=755 entrypoint.sh /usr/local/bin/entrypoint.sh
USER dev

# Must match the `develop.watch` targets of docker-compose.yml and entrypoint.sh
# change api name
WORKDIR /usr/src/template

# --- DEPENDENCY CACHE ---
COPY --chown=dev:dev Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
# This layer stays cached as long as Cargo.toml and Cargo.lock do not change
RUN cargo build --locked && rm -rf src
# -----------------------------

COPY --chown=dev:dev src ./src

# change port
EXPOSE 3000
CMD ["/usr/local/bin/entrypoint.sh"]
