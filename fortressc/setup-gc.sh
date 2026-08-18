#!/usr/bin/env bash
# Builds a private Boehm GC prefix for linking Fortress programs, without root.
#
# Fedora splits it the same way it splits LLVM: `gc` ships libgc.so.1, `gc-devel`
# ships gc.h and the unversioned .so symlink the linker needs. This unpacks
# gc-devel into ~/.local and points its symlink at the already-installed runtime
# library. With root you would just `dnf install gc-devel` instead.
set -euo pipefail

ROOT="${HOME}/.local/opt/gc-root"
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

# Restricted to the distribution repositories: a third party repo that wants a
# GPG key imported will otherwise stop and wait for an answer.
echo "downloading gc-devel..."
( cd "${WORK}" && dnf download --repo=fedora --repo=updates gc-devel >/dev/null )

echo "unpacking into ${ROOT}"
rm -rf "${ROOT}"; mkdir -p "${ROOT}"
( cd "${ROOT}" && rpm2cpio "${WORK}"/gc-devel-*.x86_64.rpm | cpio -idm --quiet )

# gc-devel's libgc.so points at a file that lives in the `gc` package, so
# unpacked on its own it dangles. Repoint it at the installed runtime.
runtime="$(ls -1 /usr/lib64/libgc.so.* 2>/dev/null | grep -E '\.so\.[0-9]+$' | sort -V | tail -1)"
[ -n "${runtime}" ] || { echo "no /usr/lib64/libgc.so.*; dnf install gc" >&2; exit 1; }
ln -sf "${runtime}" "${ROOT}/usr/lib64/libgc.so"

test -f "${ROOT}/usr/include/gc.h"
echo
echo "ok. export these before building or linking:"
echo
echo "  export CPATH=${ROOT}/usr/include\${CPATH:+:\${CPATH}}"
echo "  export LIBRARY_PATH=${ROOT}/usr/lib64\${LIBRARY_PATH:+:\${LIBRARY_PATH}}"
