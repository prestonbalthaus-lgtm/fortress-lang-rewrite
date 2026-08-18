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

int main(void) {
    fortress_runtime_init();

    void *array = build_held_array();
    GC_gcollect();
    GC_gcollect();

    size_t live = GC_get_heap_size() - GC_get_free_bytes();
    long long length = fortress_array_length(array);

    printf("payload %zu\nlive %zu\nlength %lld\n", PAYLOAD, live, length);

    if (length != COUNT) {
        fputs("FAIL the array lost its length\n", stderr);
        return 1;
    }
    if (live * 3 < PAYLOAD * 2) {
        fprintf(stderr,
                "FAIL the collector reclaimed what the array is holding: %zu live of %zu\n",
                live, PAYLOAD);
        return 1;
    }
    /* Keeps the array reachable across the measurement. */
    return (int)(length - COUNT);
}
