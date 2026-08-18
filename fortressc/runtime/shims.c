/*
 * The Fortress runtime. Every symbol here is a target the type checker resolved
 * statically, so the compiler emits a direct call and never dispatches.
 *
 * Memory: collected, by Boehm-Demers-Weiser. M1 centralised every allocation
 * behind fortress_alloc precisely so that this would be a change to one
 * function, and it was.
 *
 * fortress_alloc hands out ATOMIC memory: the collector does not scan it for
 * pointers. That is correct for strings, which are bytes, and it is what keeps
 * a million string allocations from retaining each other by accident when a
 * character pair happens to look like an address. Anything that stores a
 * pointer into the heap -- M3b's arrays, above all -- needs a scannable
 * allocator built on GC_malloc instead, or the collector will free the objects
 * it points at while it is still holding them.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(FORTRESS_NO_GC)
/*
 * The leaking allocator M1 shipped, kept only so tools/memory-gate.sh has a
 * negative control: an RSS measurement that cannot tell a collected build from
 * a leaking one is not a measurement. Nothing in the compiler defines this.
 */
#define FORTRESS_RAW_ALLOC(bytes) malloc(bytes)
#define FORTRESS_RAW_ALLOC_SCANNED(bytes) malloc(bytes)

void fortress_runtime_init(void) {}
#else
#include <gc.h>

#define FORTRESS_RAW_ALLOC(bytes) GC_malloc_atomic(bytes)
#define FORTRESS_RAW_ALLOC_SCANNED(bytes) GC_malloc(bytes)

/* Generated main calls this before anything else, so the collector is up
 * before the first allocation. */
void fortress_runtime_init(void) { GC_INIT(); }
#endif

static void *checked(void *p) {
    if (p == NULL) {
        fputs("fortress: out of memory\n", stderr);
        abort();
    }
    return p;
}

/* Pointer-free bytes. Not scanned. */
static void *fortress_alloc(size_t bytes) { return checked(FORTRESS_RAW_ALLOC(bytes)); }

/* Memory that may hold pointers, and so must be traced. */
static void *fortress_alloc_scanned(size_t bytes) {
    return checked(FORTRESS_RAW_ALLOC_SCANNED(bytes));
}

/*
 * A runtime fault the program cannot be allowed to continue past. Clean exit
 * with a diagnostic, never a segmentation fault: an out of bounds subscript is
 * a fact about the program, and it should read like one.
 */
static void fortress_halt(const char *what, long long a, long long b) {
    fprintf(stderr, "fortress: %s (%lld, %lld)\n", what, a, b);
    exit(1);
}

void println_string(const char *s) { printf("%s\n", s); }
void println_zz32(int v) { printf("%d\n", v); }
void println_zz64(long long v) { printf("%lld\n", v); }
void println_rr64(double v) { printf("%g\n", v); }
void println_boolean(int v) { puts(v ? "true" : "false"); }
void println_void(void) { puts(""); }

char *to_string_zz32(int v) {
    int n = snprintf(NULL, 0, "%d", v);
    char *out = fortress_alloc((size_t)n + 1);
    snprintf(out, (size_t)n + 1, "%d", v);
    return out;
}

char *to_string_zz64(long long v) {
    int n = snprintf(NULL, 0, "%lld", v);
    char *out = fortress_alloc((size_t)n + 1);
    snprintf(out, (size_t)n + 1, "%lld", v);
    return out;
}

char *to_string_rr64(double v) {
    int n = snprintf(NULL, 0, "%g", v);
    char *out = fortress_alloc((size_t)n + 1);
    snprintf(out, (size_t)n + 1, "%g", v);
    return out;
}

char *to_string_boolean(int v) {
    const char *s = v ? "true" : "false";
    size_t n = strlen(s);
    char *out = fortress_alloc(n + 1);
    memcpy(out, s, n + 1);
    return out;
}

char *concat_string_string(const char *a, const char *b) {
    size_t na = strlen(a);
    size_t nb = strlen(b);
    char *out = fortress_alloc(na + nb + 1);
    memcpy(out, a, na);
    memcpy(out + na, b, nb + 1);
    return out;
}

/*
 * Arrays. One dimensional, homogeneous, and one allocation: the header and the
 * elements are a single block, so an array is one object to the collector and a
 * slot is a fixed offset from its base.
 *
 * SCANNED, not atomic. An Array[\String\] holds real pointers, and the string
 * allocator above deliberately hands out memory the collector does not trace.
 * Allocating array storage there would let the collector reclaim strings the
 * array is still holding.
 */
typedef struct {
    long long length;
    long long elem_bytes;
    /* Aligned to 8 by the two fields above, which is enough for every element
     * type the language has. */
    char data[];
} FortressArray;

void *fortress_array_alloc(long long count, long long elem_bytes, int holds_pointers) {
    if (count < 0) {
        fortress_halt("array length is negative", count, elem_bytes);
    }
    if (elem_bytes <= 0 || (unsigned long long)count >
                               (SIZE_MAX - sizeof(FortressArray)) / (unsigned long long)elem_bytes) {
        fortress_halt("array is too large to allocate", count, elem_bytes);
    }

    size_t bytes = (size_t)count * (size_t)elem_bytes;
    FortressArray *a = fortress_alloc_scanned(sizeof(FortressArray) + bytes);
    a->length = count;
    a->elem_bytes = elem_bytes;

    /* Filled explicitly rather than relying on the allocator: the negative
     * control is plain malloc and does not zero, and an unwritten pointer slot
     * has to be a valid empty string rather than a null. */
    if (holds_pointers) {
        static const char empty[] = "";
        char **slots = (char **)(void *)a->data;
        for (long long i = 0; i < count; i++) {
            slots[i] = (char *)empty;
        }
    } else {
        memset(a->data, 0, bytes);
    }
    return a;
}

long long fortress_array_length(const void *array) {
    return ((const FortressArray *)array)->length;
}

/*
 * Objects. One block per instance: a 32 bit concrete type tag at offset 0, four
 * bytes of padding, then the fields at +8. The tag is written here rather than
 * in generated code so that there is exactly one place it can be written from,
 * the same reason the bounds check is in exactly one place.
 *
 * SCANNED, with no exception for an object whose fields are all scalars. An
 * object that holds a String or another object holds real pointers, and telling
 * the two cases apart at the allocation site is how the collector ends up
 * freeing something that is still reachable.
 */
void *fortress_object_alloc(long long bytes, int tag) {
    if (bytes < (long long)sizeof(int)) {
        fortress_halt("object is too small to carry a tag", bytes, tag);
    }
    void *object = fortress_alloc_scanned((size_t)bytes);
    /* The negative control is plain malloc and does not zero. A field read
     * before its store would otherwise see whatever was there. */
    memset(object, 0, (size_t)bytes);
    *(int *)object = tag;
    return object;
}

/*
 * A switch arm no concrete tag can reach. Statically dead -- the type checker
 * proved every cell has a winner before it emitted the switch -- and it exists
 * because "unreachable" should mean a clean halt with a diagnostic rather than
 * undefined behaviour.
 */
void fortress_dispatch_failed(const char *name, int position, int tag) {
    fprintf(stderr, "fortress: no declaration of %s for argument %d with type tag %d\n", name,
            position, tag);
    exit(1);
}

void *fortress_array_slot(void *array, long long index) {
    FortressArray *a = array;
    if (index < 0 || index >= a->length) {
        fortress_halt("array index out of bounds", index, a->length);
    }
    return a->data + (size_t)index * (size_t)a->elem_bytes;
}
