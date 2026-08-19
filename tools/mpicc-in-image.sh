#!/usr/bin/env bash
# A link driver that lives inside the Apptainer image.
#
# fortressc runs on the build host and knows nothing about where MPI is; it
# just runs whatever `--cc` names. Naming this script hands the link to the
# cluster image's own mpicc, so the binary is linked against the MPI and the C
# library the compute nodes actually have.
set -euo pipefail

sif=${FORTRESS_MPI_SIF:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/apptainer/fortress-mpi.sif}
if [[ ! -f $sif ]]; then
    echo "mpicc-in-image: no image at $sif; build it with apptainer/fortress-mpi.def" >&2
    exit 1
fi

# The host's CPATH and LIBRARY_PATH point at the host's gc-root, and Apptainer
# bind-mounts $HOME and passes the environment through -- so without this the
# in-image link picks up the HOST's static libgc.a. That archive is compiled
# against the build host's glibc and fails inside Rocky 9 with an undefined
# `__isoc23_strtol`. The image has its own collector; point at it.
exec env \
    APPTAINERENV_CPATH=/opt/gc-root/usr/include \
    APPTAINERENV_LIBRARY_PATH=/opt/gc-root/usr/lib64 \
    apptainer exec "$sif" mpicc "$@"
