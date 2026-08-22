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
/*
 * M4: the collector must know about every thread that allocates. Defining
 * GC_THREADS before <gc.h> is what makes gc.h pull in gc_pthread_redirects.h,
 * which redefines pthread_create as GC_pthread_create -- so a plain
 * pthread_create below registers the thread transparently.
 *
 * Without it the program aborts with the collector's own "Collecting from
 * unknown thread", every run. That is not a documented risk, it is a measured
 * one, and tools/parallel-gate.sh mutates this line to show it.
 */
/*
 * XSI, for pthread_mutexattr_settype and PTHREAD_MUTEX_RECURSIVE. Declared
 * here rather than left to the compiler's default because tools/memory-gate.sh
 * builds this file under -std=c11, which turns every glibc extension off
 * unless a feature test macro asks for it. _DEFAULT_SOURCE keeps everything
 * the ordinary gnu11 build already had.
 */
#define _XOPEN_SOURCE 700
#define _DEFAULT_SOURCE

#define GC_THREADS

#include <limits.h>
#include <math.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

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
void fortress_runtime_init(void) {
    GC_INIT();
    /* Starts the marker threads. Until this runs GC_get_parallel() reports 0
     * even on a collector built with --enable-parallel-mark, which is what
     * made an early measurement of this claim wrong. */
    GC_allow_register_threads();
}
#endif


/* ------------------------------------------------------------------ M4
 *
 * The parallel loop runtime. A fixed pool, a static split, and no atomics on
 * the hot path.
 *
 * The split is a pure function of (lo, hi, workers) and nothing else, which is
 * what lets tools/parallel-gate.sh compute the expected partition itself
 * instead of reading it back out of the thing it is testing.
 *
 * The calling thread takes chunk 0 and runs it itself, so P-way parallelism
 * costs P-1 spawned threads and a one-worker machine spawns none.
 */

#define FORTRESS_MAX_WORKERS 16
/* Below this, the synchronisation costs more than the work it distributes. A
 * parallel loop slower than a serial one is a bug, not a tradeoff. */
#define FORTRESS_PARALLEL_MIN 4096

/*
 * M5 appended `chunk`, and appending is the point: putting the worker index
 * second would renumber `env` from get_nth_param(1) to (2) in codegen, and
 * get_nth_param returns an Option -- a wrong index is a run-time internal
 * error rather than a compile error. Every serial path passes 0, so one
 * lowering serves the distributed and the sequential case.
 */
typedef void (*fortress_loop_body)(int64_t index, void *env, int64_t chunk);

/* The chunk worker `w` of `workers` owns. Contiguous, and every index in
 * [lo, hi) belongs to exactly one worker: the first `remainder` chunks take one
 * extra iteration, so no index is dropped and none is run twice. */
static void fortress_chunk(int64_t lo, int64_t hi, int w, int workers,
                           int64_t *start, int64_t *end) {
    int64_t total = hi - lo;
    int64_t base = total / workers;
    int64_t extra = total % workers;
    int64_t begin = lo + base * w + (w < extra ? w : extra);
    int64_t count = base + (w < extra ? 1 : 0);
    *start = begin;
    *end = begin + count;
}

struct fortress_task {
    int64_t lo, hi;
    fortress_loop_body body;
    void *env;
    int workers;
};

static pthread_t fortress_pool[FORTRESS_MAX_WORKERS];
static pthread_mutex_t fortress_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t fortress_go = PTHREAD_COND_INITIALIZER;
static pthread_cond_t fortress_done = PTHREAD_COND_INITIALIZER;
static struct fortress_task fortress_task;
static int fortress_pool_size = 0;   /* spawned threads; parallelism is this + 1 */
static unsigned long fortress_generation = 0;
static int fortress_outstanding = 0;
static int fortress_stopping = 0;

/* Set inside a worker, and on the calling thread while a loop is running. A
 * nested parallel loop runs sequentially: one pool, one level. */
static __thread int fortress_in_parallel = 0;

static void fortress_run_chunk(const struct fortress_task *task, int w) {
    int64_t start, end;
    fortress_chunk(task->lo, task->hi, w, task->workers, &start, &end);
    for (int64_t i = start; i < end; i++) {
        task->body(i, task->env, w);
    }
}

static void *fortress_worker(void *arg) {
    int id = (int)(intptr_t)arg;
    unsigned long seen = 0;

    fortress_in_parallel = 1;
    for (;;) {
        struct fortress_task task;
        pthread_mutex_lock(&fortress_lock);
        while (fortress_generation == seen && !fortress_stopping) {
            pthread_cond_wait(&fortress_go, &fortress_lock);
        }
        if (fortress_stopping) {
            pthread_mutex_unlock(&fortress_lock);
            return NULL;
        }
        seen = fortress_generation;
        task = fortress_task;
        pthread_mutex_unlock(&fortress_lock);

        /* Worker `id` runs chunk id+1; chunk 0 belongs to the caller. */
        if (id + 1 < task.workers) {
            fortress_run_chunk(&task, id + 1);
        }

        pthread_mutex_lock(&fortress_lock);
        if (--fortress_outstanding == 0) {
            pthread_cond_signal(&fortress_done);
        }
        pthread_mutex_unlock(&fortress_lock);
    }
}

static void fortress_pool_stop(void) {
    pthread_mutex_lock(&fortress_lock);
    fortress_stopping = 1;
    pthread_cond_broadcast(&fortress_go);
    pthread_mutex_unlock(&fortress_lock);
    for (int i = 0; i < fortress_pool_size; i++) {
        pthread_join(fortress_pool[i], NULL);
    }
    fortress_pool_size = 0;
}

/*
 * N threads allocate N times faster, so with an untuned heap they collect N
 * times as often -- and every collection is stop the world. Measured on this
 * machine: an allocating loop that should scale 3.3x runs at 1.14x with the
 * default heap policy and 703 collections, and at 3.28x with 6.
 *
 * So the heap grows with the worker count, once, on first parallel use. A
 * program with no parallel loop keeps exactly the memory behaviour it had,
 * which is also what keeps tools/memory-gate.sh measuring something.
 */
static void fortress_parallel_heap(int parallelism) {
#if !defined(FORTRESS_NO_GC)
    size_t megabytes = (size_t)parallelism * 8;
    const char *override = getenv("FORTRESS_GC_HEAP_MB");
    if (override != NULL) {
        megabytes = (size_t)strtoul(override, NULL, 10);
    } else if (megabytes > 64) {
        megabytes = 64;
    }
    GC_set_free_space_divisor(1);
    if (megabytes > 0) {
        GC_expand_hp(megabytes * 1024u * 1024u);
    }
#else
    (void)parallelism;
#endif
}

static int fortress_pool_start(void) {
    long cores = sysconf(_SC_NPROCESSORS_ONLN);
    int parallelism = cores > 0 ? (int)cores : 1;
    if (parallelism > FORTRESS_MAX_WORKERS) {
        parallelism = FORTRESS_MAX_WORKERS;
    }
    const char *override = getenv("FORTRESS_WORKERS");
    if (override != NULL) {
        long want = strtol(override, NULL, 10);
        if (want >= 1 && want <= FORTRESS_MAX_WORKERS) {
            parallelism = (int)want;
        }
    }

    fortress_parallel_heap(parallelism);

    for (int i = 0; i < parallelism - 1; i++) {
        /* GC_pthread_create, via the redirect GC_THREADS turns on. A raw
         * pthread_create here aborts the program on the first collection. */
        if (pthread_create(&fortress_pool[i], NULL, fortress_worker,
                           (void *)(intptr_t)i) != 0) {
            /* Whatever started is still usable; run with what we have. */
            fortress_pool_size = i;
            return i + 1;
        }
    }
    fortress_pool_size = parallelism - 1;
    atexit(fortress_pool_stop);
    return parallelism;
}

/*
 * Run `body(i, env)` for every i in [lo, hi). The body is an outlined function
 * and `env` is its captured environment, allocated ONCE by the caller -- never
 * per iteration, because allocation inside the parallel region is what the
 * heap measurement above is about.
 */
void fortress_parallel_for(int64_t lo, int64_t hi, fortress_loop_body body, void *env,
                           int64_t requested) {
    static int parallelism = 0;
    struct fortress_task task;

    if (hi <= lo) {
        return;
    }

    /* `requested == 1` is `seq(...)`: a promise about ORDER, so it is honoured
     * whatever the range size. Everything else runs here too when it is too
     * small to be worth distributing, or when a parallel loop is already
     * running -- one pool, one level. */
    if (requested == 1 || hi - lo < FORTRESS_PARALLEL_MIN || fortress_in_parallel) {
        for (int64_t i = lo; i < hi; i++) {
            body(i, env, 0);
        }
        return;
    }

    if (parallelism == 0) {
        parallelism = fortress_pool_start();
    }
    if (parallelism <= 1) {
        for (int64_t i = lo; i < hi; i++) {
            body(i, env, 0);
        }
        return;
    }

    task.lo = lo;
    task.hi = hi;
    task.body = body;
    task.env = env;
    task.workers = parallelism;

    pthread_mutex_lock(&fortress_lock);
    fortress_task = task;
    fortress_outstanding = fortress_pool_size;
    fortress_generation++;
    pthread_cond_broadcast(&fortress_go);
    pthread_mutex_unlock(&fortress_lock);

    /* The caller is worker 0 and does a chunk's worth of the work itself. */
    fortress_in_parallel = 1;
    fortress_run_chunk(&task, 0);
    fortress_in_parallel = 0;

    pthread_mutex_lock(&fortress_lock);
    while (fortress_outstanding > 0) {
        pthread_cond_wait(&fortress_done, &fortress_lock);
    }
    pthread_mutex_unlock(&fortress_lock);
}

/* The gate computes the same split independently and compares. */
void fortress_parallel_chunk(int64_t lo, int64_t hi, int w, int workers,
                             int64_t *start, int64_t *end) {
    fortress_chunk(lo, hi, w, workers, start, end);
}

/* ------------------------------------------------------------------ M5
 *
 * `atomic`. One process-wide RECURSIVE mutex. atomic.tex:89-90 leaves the
 * serialization mechanism to the implementation and the reference
 * implementation was a global lock underneath as well -- Transaction.java's
 * one AtomicInteger, CASed above the nested-commit branch.
 *
 * RECURSIVE is measured, not defensive. atomic.tex:72-75 permits arbitrary
 * nesting, tests/atomic4.fss nests two, and a PTHREAD_MUTEX_DEFAULT
 * self-deadlocks on the inner acquisition.
 *
 * THE SECOND HALF IS THE `fortress_in_parallel` HANDOFF, and without it an
 * `atomic` written around a parallel loop is a HARD DEADLOCK: the inner loop
 * really distributes, the workers block on the mutex the calling thread holds,
 * and the calling thread parks at the join below. Recursion does not rescue
 * that -- recursion rescues re-entry by the SAME thread and the workers are
 * different threads. Setting the flag makes any loop reached from inside an
 * atomic region run inline, which is also exactly what atomic.tex:77-81 asks
 * for when it requires implicit threads created inside an atomic to finish
 * before it does.
 *
 * The flag is SAVED and RESTORED rather than zeroed: an atomic taken inside a
 * worker already has it set, and clearing it on the way out would let a later
 * nested loop reach the pool from inside a running one.
 */
static pthread_mutex_t fortress_atomic_mutex;
static pthread_once_t fortress_atomic_once = PTHREAD_ONCE_INIT;
static __thread int fortress_atomic_depth = 0;
static __thread int fortress_atomic_outer_parallel = 0;

static void fortress_atomic_init(void) {
    pthread_mutexattr_t attr;
    pthread_mutexattr_init(&attr);
    pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_RECURSIVE);
    pthread_mutex_init(&fortress_atomic_mutex, &attr);
    pthread_mutexattr_destroy(&attr);
}

void fortress_atomic_enter(void) {
    pthread_once(&fortress_atomic_once, fortress_atomic_init);
    pthread_mutex_lock(&fortress_atomic_mutex);
    if (fortress_atomic_depth++ == 0) {
        fortress_atomic_outer_parallel = fortress_in_parallel;
        fortress_in_parallel = 1;
    }
}

void fortress_atomic_leave(void) {
    if (--fortress_atomic_depth == 0) {
        fortress_in_parallel = fortress_atomic_outer_parallel;
    }
    pthread_mutex_unlock(&fortress_atomic_mutex);
}

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
 * The captured environment of an outlined loop body. SCANNED, because a capture
 * may be a String or an Array and the collector has to see through the
 * environment to it while a worker still holds it. Allocated once per loop, by
 * the code that starts the loop, never per iteration.
 */
void *fortress_env_alloc(int64_t bytes) {
    if (bytes <= 0) {
        return NULL;
    }
    return fortress_alloc_scanned((size_t)bytes);
}

/*
 * How many rows fortress_reduction_alloc lays down, and therefore how many the
 * merge folds. Exported rather than duplicated as a constant in codegen: a
 * worker writes row `chunk` and chunk is at most workers-1, so the two numbers
 * disagreeing is an out of bounds store from a thread.
 */
int64_t fortress_reduction_workers(void) { return FORTRESS_MAX_WORKERS; }

/*
 * The per-worker accumulators, one scanned-free block of workers x reductions
 * slots, allocated once beside the environment and never touched again by this
 * file -- codegen owns the arithmetic.
 *
 * ATOMIC, not scanned, and the checker is what makes that safe: a reduction
 * variable is ZZ32, ZZ64 or RR64 and nothing else, so no slot ever holds a
 * pointer. Widening the reduction set to a reference type has to come back
 * through this allocator.
 *
 * `stride` comes from the CALLER so the two sides cannot disagree about the
 * padding. It is a cacheline, and that is measured rather than hygiene: 20M
 * updates, padded against a plain int64_t[16], best of 3 -- 0.0055 vs 0.0078
 * at 8 workers and 0.0036 vs 0.0093 at 14, and the unpadded one gets WORSE
 * from 8 workers to 14.
 */
void *fortress_reduction_alloc(int64_t reductions, int64_t stride) {
    if (reductions <= 0 || stride <= 0) {
        return NULL;
    }
    size_t bytes = (size_t)FORTRESS_MAX_WORKERS * (size_t)reductions * (size_t)stride;
    void *block = fortress_alloc(bytes);
    /* Identity for `+` and `-` on all three reducible types is a zero bit
     * pattern. Written explicitly: the atomic allocator does not zero, and
     * neither does the FORTRESS_NO_GC negative control. */
    memset(block, 0, bytes);
    return block;
}

/*
 * A runtime fault the program cannot be allowed to continue past. Clean exit
 * with a diagnostic, never a segmentation fault: an out of bounds subscript is
 * a fact about the program, and it should read like one.
 *
 * _exit AND NOT exit, and it is M5 that makes the difference load bearing.
 * fortress_pool_stop is an atexit handler whose body is pthread_join over the
 * pool, and a worker parked in fortress_atomic_enter on a mutex this thread
 * still holds can never be joined -- the diagnostic prints and the process
 * hangs forever, which under srun is a job burning its whole allocation. An
 * abnormal halt has no business running atexit handlers, so it does not.
 * fflush(NULL) first, because _exit does not flush stdio either.
 */
static void fortress_abnormal_exit(void) {
    fflush(NULL);
    _exit(1);
}

static void fortress_halt(const char *what, long long a, long long b) {
    fprintf(stderr, "fortress: %s (%lld, %lld)\n", what, a, b);
    fortress_abnormal_exit();
}

/*
 * Integer division. TWO of an `sdiv`'s operand pairs fault on x86-64 rather
 * than producing a value -- a zero divisor, and the minimum value over -1,
 * whose quotient is not representable -- and both raise SIGFPE. That is a core
 * dump with no diagnostic, and it takes whatever stdio had buffered with it, so
 * a program loses output it had already produced. 1.0 throws DivisionByZero;
 * this subset has no exceptions, so division halts the way a bad subscript
 * does. RR64 division is NOT routed here: 1.0/0.0 is `inf` and that is right.
 *
 * The exception is spelled DivisionByZero -- `opr-overview.tex:164-170` and
 * `Library/FortressLibrary.fss:1459`, an UncheckedException. `DivideByZero`
 * appears nowhere in either spec tree, and `IntegerDivisionByZero`
 * (`basic-integers.tex:459`) is declared nowhere at all. Note also that 1.0's
 * `/` on integers yields a RATIONAL and does not throw; the throw belongs to
 * the division that stays in the integers, which is what fortressc's `/` is.
 */
long long fortress_div_zz64(long long a, long long b) {
    if (b == 0) {
        fortress_halt("integer division by zero", a, b);
    }
    if (a == LLONG_MIN && b == -1) {
        fortress_halt("integer division overflows", a, b);
    }
    return a / b;
}

/*
 * The 32 bit width delegates, so the zero rule is written down once. The
 * overflow rule cannot delegate: INT_MIN / -1 is representable as a long long
 * and would come back truncated to INT_MIN instead of halting, which is the
 * silently wrong answer the guard exists to prevent. Every other quotient of
 * two ints fits in an int, so the cast is lossless.
 */
int fortress_div_zz32(int a, int b) {
    if (a == INT_MIN && b == -1) {
        fortress_halt("integer division overflows", a, b);
    }
    return (int)fortress_div_zz64(a, b);
}

void println_string(const char *s) { printf("%s\n", s); }
void println_zz32(int v) { printf("%d\n", v); }
void println_zz64(long long v) { printf("%lld\n", v); }
/*
 * A Fortress RR64 always shows that it is one: `17.0`, never `17`. C's "%g"
 * drops a trailing ".0" and that is a SILENT WRONG ANSWER rather than a
 * formatting preference -- `compiler_tests/Compiled7.Print17.fss` asserts
 * `17.0` for `(23.0 - 6.0).asString` and we printed `17`.
 *
 * THREE SHIMS REACHED %g AND ALL THREE WERE WRONG: `println`, `print` and
 * `to_string`. They share this predicate so they cannot drift apart -- a value
 * printed one way and concatenated another is the defect one step later.
 *
 * Anything already carrying a `.`, an exponent, or a nan/inf spelling is left
 * exactly as "%g" wrote it.
 */
static int rr64_needs_point(const char *text) {
    for (const char *p = text; *p != '\0'; p++) {
        if (*p == '.' || *p == 'e' || *p == 'E' || *p == 'n' || *p == 'i') {
            return 0;
        }
    }
    return 1;
}

void println_rr64(double v) {
    char buf[64];
    snprintf(buf, sizeof buf, "%g", v);
    printf(rr64_needs_point(buf) ? "%s.0\n" : "%s\n", buf);
}
void println_boolean(int v) { puts(v ? "true" : "false"); }
void println_void(void) { puts(""); }

/*
 * `print` is `println` without the newline. Separate shims rather than a flag,
 * because a flag would be one more thing generated code has to get right and
 * these are four lines each.
 */
/*
 * `^`. There is no integer power instruction, so this is a shim rather than
 * inline IR -- and being a shim is what keeps the negative-exponent rule in one
 * place. A negative exponent on an integer has no integer answer, so it halts
 * the way an out of bounds subscript does rather than inventing zero.
 *
 * Exponentiation by squaring: the loop is O(log b), and it is the same shape
 * for both integer widths.
 */
long long pow_zz64_zz64(long long a, long long b) {
    if (b < 0) {
        fortress_halt("negative exponent has no integer result", a, b);
    }
    long long result = 1;
    long long base = a;
    while (b > 0) {
        if (b & 1) {
            result *= base;
        }
        b >>= 1;
        if (b > 0) {
            base *= base;
        }
    }
    return result;
}

int pow_zz32_zz32(int a, int b) { return (int)pow_zz64_zz64(a, b); }

/*
 * The mixed pairs. 1.0 declares `^` on every base-exponent combination and
 * expTest.fss asserts all four, so all nine exist rather than a rule about
 * which ones are allowed. A real anywhere makes the answer real.
 */
double pow_rr64_rr64(double a, double b) { return pow(a, b); }
double pow_rr64_zz32(double a, int b) { return pow(a, (double)b); }
double pow_rr64_zz64(double a, long long b) { return pow(a, (double)b); }
double pow_zz32_rr64(int a, double b) { return pow((double)a, b); }
double pow_zz64_rr64(long long a, double b) { return pow((double)a, b); }
int pow_zz32_zz64(int a, long long b) { return (int)pow_zz64_zz64(a, b); }
long long pow_zz64_zz32(long long a, int b) { return pow_zz64_zz64(a, b); }

void print_string(const char *s) { fputs(s, stdout); }
void print_zz32(int v) { printf("%d", v); }
void print_zz64(long long v) { printf("%lld", v); }
void print_rr64(double v) {
    char buf[64];
    snprintf(buf, sizeof buf, "%g", v);
    printf(rr64_needs_point(buf) ? "%s.0" : "%s", buf);
}
void print_boolean(int v) { fputs(v ? "true" : "false", stdout); }
void print_void(void) {}

/*
 * A failed `assert`. Fortress 1.0 throws; there are no exceptions here, so it
 * halts the way an out of bounds subscript does -- a diagnostic on stderr and
 * exit 1, never a silent continue.
 */
void fortress_assert_failed(const char *message) {
    fflush(stdout);
    fprintf(stderr, "fortress: assertion failed: %s\n", message);
    fortress_abnormal_exit();
}

/*
 * No arm of a `case` matched and the source wrote no `else`. 1.0 throws
 * MatchFailure here (case-expr.tex); this subset has no exceptions, so it
 * halts the way every other cannot-continue does -- a diagnostic and exit 1,
 * never a silent fall through. `fortress_abnormal_exit` is _exit and not exit
 * for the reason atomic-gate mutation 2 measured: the atexit handler joins a
 * pool whose worker may be parked on a mutex this thread holds.
 */
void fortress_case_failed(void) {
    fflush(stdout);
    fputs("fortress: no case arm matched and there is no `else`\n", stderr);
    fortress_abnormal_exit();
}

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

/* See `rr64_needs_point`. */
char *to_string_rr64(double v) {
    int n = snprintf(NULL, 0, "%g", v);
    if (n < 0) {
        return fortress_alloc(1);
    }
    size_t len = (size_t)n;
    char *body = fortress_alloc(len + 1);
    snprintf(body, len + 1, "%g", v);
    if (rr64_needs_point(body) == 0) {
        return body;
    }
    char *out = fortress_alloc(len + 3);
    memcpy(out, body, len);
    out[len] = '.';
    out[len + 1] = '0';
    out[len + 2] = '\0';
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
    fortress_abnormal_exit();
}

void *fortress_array_slot(void *array, long long index) {
    FortressArray *a = array;
    if (index < 0 || index >= a->length) {
        fortress_halt("array index out of bounds", index, a->length);
    }
    return a->data + (size_t)index * (size_t)a->elem_bytes;
}

/*
 * Arrays of rank two and above. A SEPARATE STRUCT AND A SEPARATE PAIR OF SHIMS,
 * deliberately: rank one keeps the layout and the entry points above, byte for
 * byte, so every module that compiled before this milestone lowers to the same
 * IR it lowered to then. Generalising the rank-one signature would have moved
 * all of it.
 *
 * ONE ALLOCATION, as before: the header, then `rank` extents, then the
 * elements. `extents` is 8-aligned by the three fields above it and each entry
 * is 8 bytes, so the element block behind it is 8-aligned too -- which is what
 * every element type the language has needs.
 *
 * ROW MAJOR, and the linearisation is HERE and nowhere else, for the same
 * reason the rank-one bounds check is: one place it can be wrong in.
 */
typedef struct {
    long long rank;
    long long elem_bytes;
    long long total;
    /* `rank` entries, then the elements. */
    long long extents[];
} FortressArrayN;

static char *arrayn_data(FortressArrayN *a) {
    return (char *)(void *)(a->extents + a->rank);
}

/* THREE NUMBERS, not two, and that is the whole point of a separate reporter.
 * `fortress_halt` prints a pair; a bound violation on rank two has to say WHICH
 * dimension as well as the index and the extent, or `(4, 3)` leaves the reader
 * to guess whether the row or the column was wrong. */
static void fortress_halt_dim(const char *what, long long dim, long long a, long long b) {
    fprintf(stderr, "fortress: %s in dimension %lld (%lld, %lld)\n", what, dim, a, b);
    fortress_abnormal_exit();
}

void *fortress_array_alloc_n(long long rank, const long long *extents, long long elem_bytes,
                             int holds_pointers) {
    if (rank < 2) {
        fortress_halt("array rank is below two", rank, elem_bytes);
    }
    long long total = 1;
    for (long long d = 0; d < rank; d++) {
        long long extent = extents[d];
        if (extent < 0) {
            fortress_halt_dim("array extent is negative", d, extent, rank);
        }
        /* Checked BEFORE the multiply rather than after: the product is what
         * would wrap, and a wrapped total is a short allocation that every
         * later bounds check then agrees with. */
        if (extent != 0 && total > LLONG_MAX / extent) {
            fortress_halt_dim("array is too large to allocate", d, extent, total);
        }
        total *= extent;
    }
    if (elem_bytes <= 0) {
        fortress_halt("array element size is not positive", elem_bytes, rank);
    }
    size_t header = sizeof(FortressArrayN) + (size_t)rank * sizeof(long long);
    if ((unsigned long long)total > (SIZE_MAX - header) / (unsigned long long)elem_bytes) {
        fortress_halt("array is too large to allocate", total, elem_bytes);
    }

    size_t bytes = (size_t)total * (size_t)elem_bytes;
    FortressArrayN *a = fortress_alloc_scanned(header + bytes);
    a->rank = rank;
    a->elem_bytes = elem_bytes;
    a->total = total;
    for (long long d = 0; d < rank; d++) {
        a->extents[d] = extents[d];
    }

    char *data = arrayn_data(a);
    if (holds_pointers) {
        static const char empty[] = "";
        char **slots = (char **)(void *)data;
        for (long long i = 0; i < total; i++) {
            slots[i] = (char *)empty;
        }
    } else {
        memset(data, 0, bytes);
    }
    return a;
}

/*
 * EVERY DIMENSION IS CHECKED ON ITS OWN, and that is a correctness requirement
 * rather than a nicety. Linearising first and checking the result against
 * `total` accepts `a[0,4]` on a 3 by 3 array -- the linear offset is 4, which
 * is inside 9 -- and hands back the address of `a[1,1]`. That is a silent wrong
 * answer, which is the class this project hunts.
 */
void *fortress_array_slot_n(void *array, long long rank, const long long *indices) {
    FortressArrayN *a = array;
    if (rank != a->rank) {
        fortress_halt("array subscript has the wrong rank", rank, a->rank);
    }
    long long offset = 0;
    for (long long d = 0; d < rank; d++) {
        long long index = indices[d];
        long long extent = a->extents[d];
        /* TWO STATEMENTS AND NOT ONE `||`, so that a mutation table can replace
         * the upper bound on a line of its own: the gate splits its rows on
         * `|`, and a row carrying `||` cannot be written at all. */
        if (index < 0) {
            fortress_halt_dim("array index out of bounds", d, index, extent);
        }
        if (index >= extent) {
            fortress_halt_dim("array index out of bounds", d, index, extent);
        }
        offset = offset * a->extents[d] + index;
    }
    return arrayn_data(a) + (size_t)offset * (size_t)a->elem_bytes;
}
