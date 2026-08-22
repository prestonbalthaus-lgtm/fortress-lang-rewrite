/*
 * Does the collector see what an array is holding?
 *
 * A conservative collector only keeps what it can find a pointer to. Array
 * storage is allocated scannable for exactly this reason; allocated atomically
 * it would be invisible, and the strings an Array[\String\] holds would be
 * reclaimed while the array still pointed at them.
 *
 * Reading the array back afterwards is not a test: freed memory usually still
 * reads correctly, so that passes either way. This measures the thing itself.
 * After a forced collection, with the array as the only reference, the payload
 * either survives as live bytes or it does not.
 *
 *   cc -I runtime runtime/tests/array_trace.c runtime/shims.c -lgc -o array-trace
 */
#include <gc.h>
#include <stdio.h>
#include <string.h>

void fortress_runtime_init(void);
void *fortress_array_alloc(long long count, long long elem_bytes, int holds_pointers);
void *fortress_array_slot(void *array, long long index);
void *fortress_array_alloc_n(long long rank, const long long *extents, long long elem_bytes,
                             int holds_pointers);
void *fortress_array_slot_n(void *array, long long rank, const long long *indices);
long long fortress_array_length(const void *array);
char *concat_string_string(const char *a, const char *b);

#define COUNT 1024
#define CHUNK 8192
/* Two thirds of the payload has to survive for the array to be traced at all;
 * the collector is conservative, not psychic, and a stale register may retain a
 * few blocks either way. */
#define PAYLOAD ((size_t)COUNT * CHUNK)

/* In its own frame so its locals are gone before the collection runs. Only the
 * returned array still refers to the strings. */
static void *build_held_array(void) {
    static char chunk[CHUNK];
    memset(chunk, 'x', sizeof chunk - 1);
    chunk[sizeof chunk - 1] = '\0';

    void *array = fortress_array_alloc(COUNT, (long long)sizeof(char *), 1);
    for (long long i = 0; i < COUNT; i++) {
        /* Built on the heap. A string literal is static data and would survive
         * whether the array were traced or not. */
        *(char **)fortress_array_slot(array, i) = concat_string_string(chunk, "tail");
    }
    return array;
}

/*
 * THE SAME QUESTION AT RANK TWO, and it is a SEPARATE allocator answering it:
 * `fortress_array_alloc_n` is its own function with its own header, so
 * `fortress_array_alloc` being scannable says nothing about it. Non-square, so
 * a transposed extent would run off the end rather than quietly working.
 *
 * ONE RANK PER PROCESS, and that is not tidiness. The first draft built both in
 * one run and measured the second as a DELTA over the heap left behind by the
 * first -- and the freed rank-one payload was simply reused, so the delta read
 * 393216 of 8650752 and looked exactly like a collector that had reclaimed
 * everything. An absolute measurement in a fresh process is the only one that
 * means what it says.
 */
#define ROWS 32
#define COLS 33
#define PAYLOAD2 ((size_t)ROWS * COLS * CHUNK)

static void *build_held_array_2d(void) {
    static char chunk[CHUNK];
    memset(chunk, 'y', sizeof chunk - 1);
    chunk[sizeof chunk - 1] = '\0';

    const long long extents[2] = {ROWS, COLS};
    void *array = fortress_array_alloc_n(2, extents, (long long)sizeof(char *), 1);
    for (long long i = 0; i < ROWS; i++) {
        for (long long j = 0; j < COLS; j++) {
            const long long at[2] = {i, j};
            *(char **)fortress_array_slot_n(array, 2, at) = concat_string_string(chunk, "tail");
        }
    }
    return array;
}

int main(int argc, char **argv) {
    fortress_runtime_init();

    const int rank = (argc > 1 && argv[1][0] == '2') ? 2 : 1;
    void *held;
    size_t payload;
    if (rank == 1) {
        held = build_held_array();
        payload = PAYLOAD;
    } else {
        held = build_held_array_2d();
        payload = PAYLOAD2;
    }
    GC_gcollect();
    GC_gcollect();

    size_t live = GC_get_heap_size() - GC_get_free_bytes();
    printf("rank %d\npayload %zu\nlive %zu\n", rank, payload, live);

    if (rank == 1) {
        long long length = fortress_array_length(held);
        printf("length %lld\n", length);
        if (length != COUNT) {
            fputs("FAIL the array lost its length\n", stderr);
            return 1;
        }
    }
    if (live * 3 < payload * 2) {
        fprintf(stderr,
                "FAIL the collector reclaimed what the rank %d array is holding: "
                "%zu live of %zu\n",
                rank, live, payload);
        return 1;
    }

    /* Keeps the array reachable across the measurement. */
    if (rank == 1) {
        return (int)(fortress_array_length(held) - COUNT);
    }
    const long long corner[2] = {ROWS - 1, COLS - 1};
    return *(char **)fortress_array_slot_n(held, 2, corner) == NULL ? 1 : 0;
}
