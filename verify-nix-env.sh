#!/usr/bin/env bash
# Verification script for Nix development environment
# Run this inside 'nix develop' to verify everything is set up correctly

set -e

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  pg_mentat Nix Environment Verification                        ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

FAILED=0

check_command() {
    local cmd=$1
    local name=$2
    if command -v "$cmd" &> /dev/null; then
        echo -e "${GREEN}✓${NC} $name: $(command -v "$cmd")"
        if [ -n "$3" ]; then
            echo "  Version: $($cmd $3 2>&1 | head -1)"
        fi
    else
        echo -e "${RED}✗${NC} $name: NOT FOUND"
        FAILED=$((FAILED + 1))
    fi
}

check_env_var() {
    local var=$1
    local name=$2
    if [ -n "${!var}" ]; then
        echo -e "${GREEN}✓${NC} $name: ${!var}"
    else
        echo -e "${RED}✗${NC} $name: NOT SET"
        FAILED=$((FAILED + 1))
    fi
}

check_path() {
    local path=$1
    local name=$2
    if [ -e "$path" ]; then
        echo -e "${GREEN}✓${NC} $name: $path"
    else
        echo -e "${YELLOW}⚠${NC} $name: $path (not created yet)"
    fi
}

echo "═══════════════════════════════════════════════════════════════"
echo "Checking Commands..."
echo "═══════════════════════════════════════════════════════════════"
check_command rustc "Rust compiler" "--version"
check_command cargo "Cargo" "--version"
check_command rustfmt "Rustfmt" "--version"
check_command clippy-driver "Clippy" "--version"
check_command rust-analyzer "Rust Analyzer" "--version"
check_command pg_config "PostgreSQL config" "--version"
check_command clang "Clang" "--version"
check_command llvm-config "LLVM config" "--version"
check_command pkg-config "pkg-config" "--version"

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "Checking Environment Variables..."
echo "═══════════════════════════════════════════════════════════════"
check_env_var CARGO_HOME "CARGO_HOME"
check_env_var LIBCLANG_PATH "LIBCLANG_PATH"
check_env_var LLVM_CONFIG_PATH "LLVM_CONFIG_PATH"
check_env_var LD_LIBRARY_PATH "LD_LIBRARY_PATH"
check_env_var PKG_CONFIG_PATH "PKG_CONFIG_PATH"
check_env_var PGDATA "PGDATA"
check_env_var RUST_BACKTRACE "RUST_BACKTRACE"

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "Checking Paths..."
echo "═══════════════════════════════════════════════════════════════"
check_path "$LIBCLANG_PATH/libclang.so" "libclang shared library"
check_path "$CARGO_HOME" "Cargo home directory"

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "Checking Helper Functions..."
echo "═══════════════════════════════════════════════════════════════"
if type setup-pgrx &> /dev/null; then
    echo -e "${GREEN}✓${NC} setup-pgrx command available"
else
    echo -e "${RED}✗${NC} setup-pgrx command not available"
    FAILED=$((FAILED + 1))
fi

if type test-pg16 &> /dev/null; then
    echo -e "${GREEN}✓${NC} test-pg16 command available"
else
    echo -e "${RED}✗${NC} test-pg16 command not available"
    FAILED=$((FAILED + 1))
fi

if type build-extension &> /dev/null; then
    echo -e "${GREEN}✓${NC} build-extension command available"
else
    echo -e "${RED}✗${NC} build-extension command not available"
    FAILED=$((FAILED + 1))
fi

if type install-extension &> /dev/null; then
    echo -e "${GREEN}✓${NC} install-extension command available"
else
    echo -e "${RED}✗${NC} install-extension command not available"
    FAILED=$((FAILED + 1))
fi

if type start-postgres &> /dev/null; then
    echo -e "${GREEN}✓${NC} start-postgres command available"
else
    echo -e "${RED}✗${NC} start-postgres command not available"
    FAILED=$((FAILED + 1))
fi

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "Checking cargo-pgrx..."
echo "═══════════════════════════════════════════════════════════════"
if [ -f "$HOME/.pgrx/config.toml" ]; then
    echo -e "${GREEN}✓${NC} pgrx initialized at $HOME/.pgrx/config.toml"
    echo "  PostgreSQL versions:"
    grep "^pg" "$HOME/.pgrx/config.toml" | head -5
else
    echo -e "${YELLOW}⚠${NC} pgrx not initialized yet"
    echo "  Run: setup-pgrx"
fi

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "Test Compilation..."
echo "═══════════════════════════════════════════════════════════════"
echo "Testing libclang availability..."
cat > /tmp/test_bindgen.rs << 'EOF'
fn main() {
    println!("LIBCLANG_PATH: {:?}", std::env::var("LIBCLANG_PATH"));
}
EOF

if rustc /tmp/test_bindgen.rs -o /tmp/test_bindgen 2>&1; then
    echo -e "${GREEN}✓${NC} Rust compilation works"
    /tmp/test_bindgen
    rm -f /tmp/test_bindgen /tmp/test_bindgen.rs
else
    echo -e "${RED}✗${NC} Rust compilation failed"
    FAILED=$((FAILED + 1))
fi

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "Summary"
echo "═══════════════════════════════════════════════════════════════"

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo ""
    echo "Next steps:"
    echo "  1. Run 'setup-pgrx' to install cargo-pgrx (if not done)"
    echo "  2. Run 'cd pg_mentat && cargo build' to build the extension"
    echo "  3. Run 'test-pg16' to run the test suite"
    echo ""
    echo "For more information, see NIX_SETUP.md"
    exit 0
else
    echo -e "${RED}✗ $FAILED check(s) failed${NC}"
    echo ""
    echo "Troubleshooting:"
    echo "  - Make sure you're inside 'nix develop'"
    echo "  - Try exiting and re-entering the shell"
    echo "  - Check NIX_SETUP.md for more information"
    exit 1
fi
