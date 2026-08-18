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

exec apptainer exec "$sif" mpicc "$@"
