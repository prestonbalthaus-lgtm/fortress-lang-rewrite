#!/usr/bin/env bash
# Builds a private Boehm GC prefix for linking Fortress programs, without root.
#
# M4 made this a SOURCE build. The distribution package is a shared library with
# the distribution's own configure flags; a parallel language needs to own three
# decisions about its collector, and none of them are ours to make from an RPM:
#
#   --enable-parallel-mark        mark with every core, not one
#   --enable-thread-local-alloc   per-thread allocation, not one global lock
#   --enable-static --disable-shared
#
# The last is the one that matters operationally. A shared collector in a
# private prefix means every Fortress binary needs LD_LIBRARY_PATH set to run,
# including under `srun` on a compute node. Linked statically the binary carries
# its collector and needs no environment at all.
set -euo pipefail

VERSION=8.2.8
SHA256=7649020621cb26325e1fb5c8742590d92fb48ce5c259b502faf7d9fb5dabb160
URL="https://github.com/bdwgc/bdwgc/releases/download/v${VERSION}/gc-${VERSION}.tar.gz"

ROOT="${HOME}/.local/opt/gc-root"
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

echo "downloading gc-${VERSION}..."
curl -sSL -o "${WORK}/gc.tar.gz" "${URL}"

# Verified before it is unpacked, not after. A tarball that fetched cleanly is
# not the same fact as a tarball that is the one we meant to build.
echo "${SHA256}  ${WORK}/gc.tar.gz" | sha256sum -c - >/dev/null
echo "sha256 ok"

tar xzf "${WORK}/gc.tar.gz" -C "${WORK}"

cd "${WORK}/gc-${VERSION}"
./configure \
    --prefix="${ROOT}/usr" \
    --libdir="${ROOT}/usr/lib64" \
    --enable-parallel-mark \
    --enable-thread-local-alloc \
    --enable-threads=posix \
    --enable-static --disable-shared \
    --disable-cplusplus --disable-docs \
    --with-libatomic-ops=none \
    > "${WORK}/configure.log" 2>&1

make -j"$(nproc)" > "${WORK}/make.log" 2>&1

rm -rf "${ROOT}"
make install > "${WORK}/install.log" 2>&1

# Asserted, not assumed: a configure flag that was accepted and then silently
# dropped is exactly the failure this build exists to avoid.
grep -q '#define PARALLEL_MARK 1'      include/config.h
grep -q '#define THREAD_LOCAL_ALLOC 1' include/config.h
test -f "${ROOT}/usr/lib64/libgc.a"
test -f "${ROOT}/usr/include/gc.h"
! test -f "${ROOT}/usr/lib64/libgc.so"

echo
echo "ok: static libgc.a with parallel marking and thread-local allocation."
echo "export these before building or linking:"
echo
echo "  export CPATH=${ROOT}/usr/include\${CPATH:+:\${CPATH}}"
echo "  export LIBRARY_PATH=${ROOT}/usr/lib64\${LIBRARY_PATH:+:\${LIBRARY_PATH}}"
