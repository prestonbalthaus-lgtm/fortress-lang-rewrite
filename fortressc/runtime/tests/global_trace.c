/*
 * Does the collector see what a TOP-LEVEL VALUE is holding?
 *
 * Codegen emits one LLVM global per component-level value and fills it inside
 * `main`, after `fortress_runtime_init`. A conservative collector only keeps
 * what it can find a pointer to, and if the static data segment is not scanned
 * then every top-level String and Array is freed while the program still
 * refers to it.
 *
 * Boehm registers static data as roots by default -- but the house rule is a
 * measurement, and "it read back correctly" is not one: freed memory usually
 * still reads correctly, which is the point `array_trace.c` makes about arrays.
 * This measures LIVE BYTES after a forced collection with the global as the
 * only reference.
 *
 * A `static` here is the same storage class as the `Linkage::Internal` global
 * codegen emits: both land in the module's data segment.
 *
 *   cc -I runtime runtime/tests/global_trace.c runtime/shims.c -lgc -lm \
 *      -pthread -o global-trace
 */
#include <gc.h>
#include <stdio.h>
#include <string.h>

void fortress_runtime_init(void);
void *fortress_array_alloc(long long count, long long elem_bytes, int holds_pointers);
void *fortress_array_slot(void *array, long long index);
char *concat_string_string(const char *a, const char *b);

#define COUNT 512
#define CHUNK 8192
#define PAYLOAD ((size_t)COUNT * CHUNK)

/* THE GLOBAL. Exactly what a component-level value lowers to. */
static void *top_level_value;

/* In its own frame, so its locals are gone before the collection runs and the
 * global is genuinely the only reference. */
static void fill_top_level_value(void) {
    static char chunk[CHUNK];
    memset(chunk, 'v', sizeof chunk - 1);
    chunk[sizeof chunk - 1] = '\0';

    void *array = fortress_array_alloc(COUNT, (long long)sizeof(char *), 1);
    for (long long i = 0; i < COUNT; i++) {
        /* Built on the heap: a string literal is static data and would survive
         * whatever the collector did. */
        char **slot = fortress_array_slot(array, i);
        *slot = concat_string_string(chunk, "");
    }
    top_level_value = array;
}

int main(void) {
    fortress_runtime_init();
    fill_top_level_value();
    GC_gcollect();
    GC_gcollect();

    size_t live = GC_get_heap_size() - GC_get_free_bytes();
    printf("payload %zu\nlive %zu\n", PAYLOAD, live);

    /* Two thirds has to survive: the collector is conservative, not psychic,
     * and a stale register may retain a few blocks either way. The same band
     * array_trace.c uses. */
    if (live * 3 < PAYLOAD * 2) {
        fputs("FAIL a top-level value's payload was collected\n", stderr);
        return 1;
    }
    if (top_level_value == NULL) {
        fputs("FAIL the global was cleared\n", stderr);
        return 1;
    }
    puts("GLOBAL TRACE GREEN");
    return 0;
}
