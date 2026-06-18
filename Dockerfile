# Dockerfile for pg_mentat: a Mentat Datalog database as a PostgreSQL extension
#
# Build:  docker build -t pg_mentat .
# Run:    docker run -d --name pg_mentat -p 5432:5432 pg_mentat
# Connect: psql -h localhost -U postgres
#
# The container initializes with demo data on first start.

FROM postgres:16-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates \
    gcc g++ make pkg-config \
    libssl-dev libclang-dev llvm-dev \
    postgresql-server-dev-16 \
    && rm -rf /var/lib/apt/lists/*

# Install Rust (matching rust-toolchain version)
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="/usr/local/cargo/bin:${PATH}"
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain 1.90.0

# Install cargo-pgrx matching the version in pg_mentat/Cargo.toml
RUN cargo install --locked cargo-pgrx --version '~0.17'

# Initialize pgrx with the system PostgreSQL 16
RUN cargo pgrx init --pg16 /usr/bin/pg_config

# Copy the full source tree
COPY . /build/pg_mentat
WORKDIR /build/pg_mentat/pg_mentat

# Build and install the extension into the system PostgreSQL directories
RUN cargo pgrx install --release --pg-config /usr/bin/pg_config

# ---------------------------------------------------------------------------
# Runtime stage: lean image with only PostgreSQL and the installed extension
# ---------------------------------------------------------------------------
FROM postgres:16-bookworm

# Copy the compiled extension files from the builder. The base install SQL
# is generated per-version by pgrx (pg_mentat--<version>.sql); glob it so the
# image tracks the current version instead of a pinned filename.
COPY --from=builder /usr/lib/postgresql/16/lib/pg_mentat.so \
                    /usr/lib/postgresql/16/lib/pg_mentat.so
COPY --from=builder /usr/share/postgresql/16/extension/pg_mentat.control \
                    /usr/share/postgresql/16/extension/pg_mentat.control
COPY --from=builder /usr/share/postgresql/16/extension/pg_mentat--*.sql \
                    /usr/share/postgresql/16/extension/

# Copy demo initialization script (runs on first container start)
COPY demo.sql /docker-entrypoint-initdb.d/00_demo.sql

EXPOSE 5432

# Default postgres user password for the demo (override with -e POSTGRES_PASSWORD=...)
ENV POSTGRES_HOST_AUTH_METHOD=trust
