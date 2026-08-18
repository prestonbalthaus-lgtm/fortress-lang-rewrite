/*
 * The MPI boundary, and the only file in the compiler that includes <mpi.h>.
 *
 * MPI_COMM_WORLD is a macro, and its expansion is implementation specific: a
 * pointer to an opaque struct under OpenMPI, a small integer constant under
 * MPICH. Emitting either from LLVM IR would pin the compiler to whichever MPI
 * the build host happened to have. It is named here, compiled by the cluster's
 * own mpicc, and never reaches generated code.
 *
 * Every entry point is arity zero and carries the fortress_mpi_ prefix so that
 * it cannot collide with libmpi's own MPI_* symbols, with the Fortran bindings'
 * mpi_*_ symbols, or with a user function called mpiCommRank.
 */
#include <mpi.h>
#include <stdio.h>
#include <stdlib.h>

static void require_success(int code, const char *call) {
    if (code != MPI_SUCCESS) {
        fprintf(stderr, "fortress: %s failed with MPI error %d\n", call, code);
        MPI_Abort(MPI_COMM_WORLD, code);
        abort();
    }
}

void fortress_mpi_init(void) { require_success(MPI_Init(NULL, NULL), "MPI_Init"); }

int fortress_mpi_comm_rank(void) {
    int rank = -1;
    require_success(MPI_Comm_rank(MPI_COMM_WORLD, &rank), "MPI_Comm_rank");
    return rank;
}

int fortress_mpi_comm_size(void) {
    int size = -1;
    require_success(MPI_Comm_size(MPI_COMM_WORLD, &size), "MPI_Comm_size");
    return size;
}

void fortress_mpi_finalize(void) { require_success(MPI_Finalize(), "MPI_Finalize"); }
