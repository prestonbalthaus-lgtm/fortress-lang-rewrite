#!/usr/bin/env bash
# Builds a private LLVM prefix for inkwell/llvm-sys without needing root.
#
# Fedora splits LLVM: llvm-libs ships libLLVM-NN.so, llvm-devel ships
# llvm-config and the headers. This unpacks llvm-devel into ~/.local and points
# it at the already-installed runtime libraries, so nothing is installed
# system-wide. With root you would just `dnf install llvm-devel` instead.
set -euo pipefail

LLVM_MAJOR=22
ROOT="${HOME}/.local/opt/llvm${LLVM_MAJOR}-root"
PREFIX="${ROOT}/usr/lib64/llvm${LLVM_MAJOR}"
WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT

echo "downloading llvm-devel..."
( cd "${WORK}" && dnf download "llvm-devel" >/dev/null )

echo "unpacking into ${ROOT}"
rm -rf "${ROOT}"; mkdir -p "${ROOT}"
( cd "${ROOT}" && rpm2cpio "${WORK}"/llvm-devel-*.x86_64.rpm | cpio -idm --quiet )

# llvm-config refuses to report link flags unless it can see the shared library
# in its own libdir, and Rust needs the unversioned .so names that only the
# -devel packages normally provide.
link_runtime() {
    local name="$1" target
    target="$(ls -1 /usr/lib64/"${name}".so.* 2>/dev/null | grep -E '\.so\.[0-9]+$' | sort -V | tail -1)"
    [ -n "${target}" ] || { echo "missing /usr/lib64/${name}.so.*" >&2; exit 1; }
    ln -sf "${target}" "${PREFIX}/lib64/${name}.so"
}
ln -sf "/usr/lib64/libLLVM-${LLVM_MAJOR}.so" "${PREFIX}/lib64/libLLVM-${LLVM_MAJOR}.so"
ln -sf /usr/lib64/libLLVM.so.${LLVM_MAJOR}.* "${PREFIX}/lib64/" 2>/dev/null || true
link_runtime libffi
link_runtime libstdc++

echo
"${PREFIX}/bin/llvm-config" --version >/dev/null
echo "ok. export this before building:"
echo
echo "  export LLVM_SYS_${LLVM_MAJOR}1_PREFIX=${PREFIX}"
