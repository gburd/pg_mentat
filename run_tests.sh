#!/usr/bin/env bash
set -e

# Environment setup for pg_mentat tests on NixOS
# Fixes bindgen's inability to find system headers by setting BINDGEN_EXTRA_CLANG_ARGS

export CARGO_HOME=/home/gburd/ws/pg_mentat/.cargo
export TMPDIR=/home/gburd/ws/pg_mentat/.tmp
export PATH="/home/gburd/ws/pg_mentat/.cargo/bin:$PATH"

# NixOS-specific: bindgen uses libclang directly, which doesn't know about
# the Nix clang-wrapper's include paths. We must provide them explicitly.
GLIBC_DEV="/nix/store/j8irrc0mpx029dw0rmadsjylg7h31ync-glibc-2.42-51-dev"
CLANG_RESOURCE="/nix/store/lhd6p523klfpr50wncaa0h7sxizl00yg-clang-18.1.8-lib/lib/clang/18"

# Verify paths exist
if [ ! -d "$GLIBC_DEV/include" ]; then
    echo "ERROR: glibc-dev include dir not found at $GLIBC_DEV/include"
    echo "Searching for alternatives..."
    find /nix/store -maxdepth 4 -path "*/glibc-2.42*-dev/include/stdio.h" -type f 2>/dev/null | head -5
    exit 1
fi

if [ ! -d "$CLANG_RESOURCE/include" ]; then
    echo "ERROR: clang resource include dir not found at $CLANG_RESOURCE/include"
    echo "Searching for alternatives..."
    find /nix/store -maxdepth 6 -path "*/clang-18*/lib/clang/18/include/stdarg.h" -type f 2>/dev/null | head -5
    exit 1
fi

export BINDGEN_EXTRA_CLANG_ARGS="-isystem ${GLIBC_DEV}/include -isystem ${CLANG_RESOURCE}/include"

echo "=== Environment ==="
echo "CARGO_HOME=$CARGO_HOME"
echo "TMPDIR=$TMPDIR"
echo "BINDGEN_EXTRA_CLANG_ARGS=$BINDGEN_EXTRA_CLANG_ARGS"
echo "cargo-pgrx: $(which cargo-pgrx 2>/dev/null || echo 'not found')"
echo "rustc: $(rustc --version 2>/dev/null)"
echo ""

# Verify bindgen can find headers
echo "=== Verifying bindgen can compile ==="
echo '#include <stdio.h>' > "$TMPDIR/test_bindgen.h"
if clang -fsyntax-only -isystem "${GLIBC_DEV}/include" -isystem "${CLANG_RESOURCE}/include" "$TMPDIR/test_bindgen.h" 2>&1; then
    echo "Header resolution OK"
else
    echo "ERROR: Headers still not resolvable"
    exit 1
fi
rm -f "$TMPDIR/test_bindgen.h"
echo ""

# Run tests
PG_VERSION="${1:-pg16}"
echo "=== Running cargo pgrx test $PG_VERSION ==="
cd /home/gburd/ws/pg_mentat/pg_mentat

exec cargo pgrx test "$PG_VERSION" 2>&1
