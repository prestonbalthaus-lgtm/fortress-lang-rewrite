/*
 * The M1 runtime. Every symbol here is a target the type checker resolved
 * statically, so the compiler emits a direct call and never dispatches.
 *
 * Memory: the string returning shims allocate and never free. That leak is
 * accepted for M1 by design. Every allocation goes through fortress_alloc so
 * that replacing it with Boehm or ARC is a change to one function rather than
 * a hunt through generated code.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void *fortress_alloc(size_t bytes) {
    void *p = malloc(bytes);
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
