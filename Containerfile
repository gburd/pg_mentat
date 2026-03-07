# Containerfile for building and testing pg_mentat
# Build: podman build -t pg_mentat_build -f Containerfile .
# Run:   podman run -it -v .:/workspace:Z pg_mentat_build bash

FROM fedora:43

# System dependencies in a single layer for caching.
# Includes everything needed for:
#   - Rust compilation (gcc, openssl-devel, clang-devel, llvm-devel)
#   - PostgreSQL compilation from source by pgrx init (libicu-devel, readline-devel, etc.)
#   - pg_mentat extension linking (postgresql-private-devel, postgresql-private-libs)
RUN dnf install -y \
    curl \
    openssl-devel clang-devel llvm-devel \
    postgresql-private-devel postgresql-private-libs \
    gcc gcc-c++ make pkg-config \
    bison flex readline-devel zlib-devel \
    libicu-devel \
    perl-IPC-Run perl-FindBin \
    git diffutils \
    && dnf clean all

# Install rustup with the exact toolchain the project requires (1.90.0)
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="/usr/local/cargo/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain 1.90.0 && \
    rustup component add rustfmt clippy

# Install cargo-pgrx matching the version in pg_mentat/Cargo.toml
RUN cargo install --locked cargo-pgrx --version '~0.17'

# Initialize pgrx - downloads and compiles PostgreSQL 16 from source.
# Only pg16 since that is the default feature for pg_mentat.
RUN cargo pgrx init --pg16 download

ENV PGRX_HOME=/root/.pgrx

WORKDIR /workspace

CMD ["bash"]
