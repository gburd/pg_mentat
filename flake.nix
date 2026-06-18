{
  description = "pg_mentat - Mentat Datalog database for PostgreSQL";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain matching project requirements (rust-version = "1.88" in Cargo.toml)
        rustToolchain = pkgs.rust-bin.stable."1.90.0".default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };

        # PostgreSQL version used for development/testing
        postgresql = pkgs.postgresql_16;

        # Build inputs required for pg_mentat and cargo-pgrx
        commonBuildInputs = with pkgs; [
          # Rust toolchain
          rustToolchain

          # Build tools
          pkg-config
          git

          # LLVM and Clang (required for pgrx bindgen)
          llvmPackages_18.libllvm
          llvmPackages_18.clang
          llvmPackages_18.libclang
          llvmPackages_18.lld  # LLVM linker

          # OpenSSL
          openssl
          openssl.dev

          # Other dependencies
          zlib
          readline
          icu
          gettext  # NLS support

          # Build essentials
          gnumake
          gcc
          perl
        ];

        # Additional packages for the dev shell (not needed for pure builds)
        devOnlyInputs = with pkgs; [
          postgresql
          # pg_config is a separate derivation (postgresql.pg_config) in
          # current nixpkgs, not in the default or .dev output; pgrx needs it
          # on PATH for `cargo pgrx init/package/install`.
          postgresql.pg_config
          bison
          flex
        ];

        # pkg-config search path
        pkgConfigPath = pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" [
          pkgs.openssl
          pkgs.zlib
          pkgs.readline
          pkgs.icu
        ];

        # Environment variables shared between devShell and derivations
        buildEnv = {
          LIBCLANG_PATH = "${pkgs.llvmPackages_18.libclang.lib}/lib";
          LLVM_CONFIG_PATH = "${pkgs.llvmPackages_18.libllvm.dev}/bin/llvm-config";
          # Critical for bindgen: tell it where to find C standard library headers
          # Use stdenv.cc.libc which has the complete glibc setup
          BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.stdenv.cc.libc.dev}/include -isystem ${pkgs.llvmPackages_18.libclang.lib}/lib/clang/18/include";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
            pkgs.llvmPackages_18.libllvm
            pkgs.llvmPackages_18.libclang
            pkgs.openssl
            pkgs.zlib
            pkgs.readline
            pkgs.icu
          ];
          PKG_CONFIG_PATH = pkgConfigPath;
        };

        # Extra environment variables for the dev shell only
        devEnv = buildEnv // {
          RUST_BACKTRACE = "1";
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          CARGO_HOME = "${toString ./.}/.cargo";
          PGDATA = "${toString ./.}/.postgres-data";
        };

      in
      {
        # Development shell
        devShells.default = pkgs.mkShell {
          buildInputs = commonBuildInputs ++ devOnlyInputs;

          shellHook = ''
            ${pkgs.lib.concatStringsSep "\n"
              (pkgs.lib.mapAttrsToList (name: value: "export ${name}=\"${toString value}\"") devEnv)}

            # CARGO_HOME / PGDATA from devEnv resolve to the nix-store copy of
            # the flake source, which is read-only -- cargo install then fails
            # with EACCES. Repoint them at the real working directory at shell
            # entry so they are writable (CI checks out into a writable dir).
            export CARGO_HOME="$PWD/.cargo"
            export PGDATA="$PWD/.postgres-data"

            # Create cargo home directory
            mkdir -p "$CARGO_HOME"

            # Helper: install and initialize cargo-pgrx
            setup-pgrx() {
              echo "Installing cargo-pgrx 0.17..."
              cargo install --locked cargo-pgrx --version '~0.17'
              echo "Initializing pgrx with the dev-shell PostgreSQL 16..."
              cargo pgrx init --pg16="${postgresql.pg_config}/bin/pg_config"
              echo "pgrx setup complete."
            }

            # Helper: run tests against PostgreSQL 16. cargo pgrx test takes
            # only [PG_VERSION] [TESTNAME] + options (no libtest `--`
            # passthrough), so an optional name filter is the only argument.
            test-pg16() {
              (cd pg_mentat && cargo pgrx test pg16 --no-schema "$@")
            }

            # Helper: build extension in release mode
            build-extension() {
              (cd pg_mentat && cargo pgrx package --pg-config="${postgresql.pg_config}/bin/pg_config")
            }

            # Helper: install extension to local PostgreSQL
            install-extension() {
              (cd pg_mentat && cargo pgrx install --release --pg-config="${postgresql.pg_config}/bin/pg_config")
            }

            # Helper: start a local PostgreSQL instance
            start-postgres() {
              if [ ! -d "$PGDATA" ]; then
                echo "Initializing PostgreSQL data directory at $PGDATA..."
                initdb -D "$PGDATA" --no-locale --encoding=UTF8
              fi
              echo "Starting PostgreSQL..."
              pg_ctl -D "$PGDATA" -l "$PGDATA/server.log" start
              echo "PostgreSQL running. Stop with: pg_ctl -D \"$PGDATA\" stop"
            }

            # Welcome message
            echo "pg_mentat development environment"
            echo ""
            echo "Rust:       $(rustc --version)"
            echo "Cargo:      $(cargo --version)"
            echo "PostgreSQL: $(pg_config --version)"
            echo ""
            echo "Environment:"
            echo "  CARGO_HOME=$CARGO_HOME"
            echo ""
            echo "Commands:"
            echo "  setup-pgrx          Install and initialize cargo-pgrx"
            echo "  test-pg16 [args]    Run tests against PostgreSQL 16"
            echo "  build-extension     Package the extension"
            echo "  install-extension   Install to local PostgreSQL"
            echo "  start-postgres      Start a local PostgreSQL instance"
            echo ""

            # Export the helper functions so they are available in
            # `nix develop --command bash -c '...'` child shells (used by CI).
            export -f setup-pgrx test-pg16 build-extension install-extension start-postgres
          '';
        };

        # Build the pg_mentat extension
        packages = {
          default = self.packages.${system}.pg_mentat;

          # NOTE: This derivation needs network access for cargo fetches.
          # Build with: nix build --option sandbox false
          # Or use the dev shell for interactive builds: nix develop
          pg_mentat = pkgs.stdenv.mkDerivation {
            pname = "pg_mentat";
            version = "1.2.1";

            src = ./.;

            nativeBuildInputs = commonBuildInputs ++ [ postgresql postgresql.pg_config ];

            inherit (buildEnv) LIBCLANG_PATH LLVM_CONFIG_PATH LD_LIBRARY_PATH PKG_CONFIG_PATH;

            # Network access required for cargo fetches
            __noChroot = true;

            buildPhase = ''
              export CARGO_HOME=$(mktemp -d)
              cargo install --locked cargo-pgrx --version '~0.17'
              cargo pgrx init --pg16="${postgresql.pg_config}/bin/pg_config"
              cd pg_mentat
              cargo pgrx package --pg-config="${postgresql.pg_config}/bin/pg_config"
            '';

            installPhase = ''
              mkdir -p $out/lib
              mkdir -p $out/share/postgresql/extension

              # Copy the compiled shared library
              find target -name 'pg_mentat.so' -exec cp {} $out/lib/ \;

              # Copy SQL files and control file
              cp pg_mentat/sql/*.sql $out/share/postgresql/extension/ || true
              cp pg_mentat/pg_mentat.control $out/share/postgresql/extension/
            '';

            meta = with pkgs.lib; {
              description = "Mentat Datalog database for PostgreSQL";
              homepage = "https://github.com/gburd/pg_mentat";
              license = licenses.asl20;
              platforms = platforms.linux;
            };
          };
        };

        # Checks for CI/CD -- validate the build compiles
        checks = {
          build = self.packages.${system}.pg_mentat;
        };
      }
    );
}
