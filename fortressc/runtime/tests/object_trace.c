/*
 * Does the collector see what a MUTABLE FIELD is holding?
 *
 * The same question `array_trace.c` asks of array storage, one storage kind
 * later. A field store is the first write in this language that puts a pointer
 * into a block that was already allocated, so if an object block were ever
 * allocated atomically the string a field names would be reclaimed while the
 * field still pointed at it -- and reading the field back would not show it,
 * because freed memory usually still reads correctly.
 *
 * The Fortress-level fixture (fortressc/tests/gcfield.fss) does NOT settle
 * this: it was run against `fortress_object_alloc` switched to the atomic
 * allocator and printed the right string five times out of five. One object
 * and one string are inside what a conservative stack scan retains anyway.
 * That is why this measures live bytes after a forced collection instead.
 *
 *   cc -Wall -Wextra -std=c11 -I runtime runtime/tests/object_trace.c \
 *      runtime/shims.c -lgc -lm -o object-trace
 */
#include <gc.h>
#include <stdio.h>
#include <string.h>

void fortress_runtime_init(void);
void *fortress_object_alloc(long long bytes, int tag);
void *fortress_array_alloc(long long count, long long elem_bytes, int holds_pointers);
void *fortress_array_slot(void *array, long long index);
long long fortress_array_length(const void *array);
char *concat_string_string(const char *a, const char *b);

#define COUNT 1024
#define CHUNK 8192
#define PAYLOAD ((size_t)COUNT * CHUNK)

/* The layout generated code emits: a 32-bit tag at offset 0 and its pad, then
 * the fields. One pointer field puts the field at offset 8. */
#define FIELD_AT 8
#define OBJECT_BYTES 16

/* In its own frame, so no local of this function is still naming a string when
 * the collection runs. The array names the objects; each object's FIELD is the
 * only thing naming its string, and the field was written AFTER the block was
 * allocated, which is the path under test. */
static void *build_holders(void) {
    static char chunk[CHUNK];
    memset(chunk, 'x', sizeof chunk - 1);
    chunk[sizeof chunk - 1] = '\0';

    void *array = fortress_array_alloc(COUNT, (long long)sizeof(char *), 1);
    for (long long i = 0; i < COUNT; i++) {
        void *holder = fortress_object_alloc(OBJECT_BYTES, 1);
        /* Built on the heap: a literal is static data and would survive
         * whether the object block were traced or not. */
        *(char **)((char *)holder + FIELD_AT) = concat_string_string(chunk, "tail");
        *(void **)fortress_array_slot(array, i) = holder;
    }
    return array;
}

int main(void) {
    fortress_runtime_init();

    void *array = build_holders();
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
                "FAIL the collector reclaimed what the fields are holding: %zu live of %zu\n",
                live, PAYLOAD);
        return 1;
    }
    return (int)(length - COUNT);
}
