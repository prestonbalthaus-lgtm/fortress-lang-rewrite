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

void fortress_runtime_init(void) {}
#else
#include <gc.h>

#define FORTRESS_RAW_ALLOC(bytes) GC_malloc_atomic(bytes)

/* Generated main calls this before anything else, so the collector is up
 * before the first allocation. */
void fortress_runtime_init(void) { GC_INIT(); }
#endif

static void *fortress_alloc(size_t bytes) {
    void *p = FORTRESS_RAW_ALLOC(bytes);
    if (p == NULL) {
        fputs("fortress: out of memory\n", stderr);
        abort();
    }
    return p;
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
